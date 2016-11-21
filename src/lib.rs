// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! jsonrpc server over unix sockets
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate json_tcp_server;
//! extern crate rand;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use json_tcp_server::Server;
//! use std::net::SocketAddr;
//! use std::str::FromStr;
//!
//! struct SayHello;
//! impl SyncMethodCommand for SayHello {
//! 	fn execute(&self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//!
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let server = Server::new(&SocketAddr::from_str("0.0.0.0:9993").unwrap(), &Arc::new(io)).unwrap();
//!     ::std::thread::spawn(move || server.run());
//! }
//! ```

extern crate jsonrpc_core;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate env_logger;
extern crate slab;
extern crate mio;
extern crate bytes;
extern crate serde_json;

#[cfg(test)]
extern crate rand;

mod validator;
#[cfg(test)]
pub mod tests;

use mio::*;
use mio::tcp::*;
use bytes::{Buf, ByteBuf, MutByteBuf};
use std::io;
use jsonrpc_core::{IoHandler, IoSession, IoSessionHandler};
use std::sync::*;
use std::sync::atomic::*;
use std::collections::VecDeque;
use std::env;
use log::LogLevelFilter;
use env_logger::LogBuilder;
use std::net::SocketAddr;
use std::collections::HashMap;

lazy_static! {
	static ref LOG_DUMMY: bool = {
		let mut builder = LogBuilder::new();
		builder.filter(None, LogLevelFilter::Info);

		if let Ok(log) = env::var("RUST_LOG") {
			builder.parse(&log);
		}

		if let Ok(_) = builder.init() {
			println!("logger initialized");
		}
		true
	};
}

/// Intialize log with default settings
pub fn init_log() {
    let _ = *LOG_DUMMY;
}

const SERVER: Token = Token(0);
const MAX_CONCURRENT_CONNECTIONS: usize = 1024;
const MAX_WRITE_LENGTH: usize = 8192;
const MAX_MESSAGES_DISPATCH: usize = 128; // per max of POLL_INTERVAL
const POLL_INTERVAL: usize = 100;

struct SocketConnection {
    socket: TcpStream,
    session: IoSession,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>,
    token: Option<Token>,
    interest: EventSet,
    addr: SocketAddr,
}

type Slab<T> = slab::Slab<T, Token>;

impl SocketConnection {
    fn new(sock: TcpStream, addr: SocketAddr, session: IoSession) -> Self {
        SocketConnection {
            socket: sock,
            session: session,
            buf: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(4096)),
            token: None,
            interest: EventSet::hup(),
            addr: addr,
        }
    }

    fn send(&mut self, event_loop: &mut EventLoop<RpcServer>, data: &[u8]) -> io::Result<()> {
        if let Some(buf) = self.buf.take() {
            let mut mut_buf = buf.flip();
            mut_buf.write_slice(data);
            self.buf = Some(mut_buf.flip());
        }
        else {
            self.buf = Some(ByteBuf::from_slice(data));
        }
        trace!(target: "tcp", "Received socket payload: {} bytes", data.len());
        self.interest.insert(EventSet::writable());
        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }

    fn writable(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
        use std::io::Write;
        if let Some(buf) = self.buf.take() {
            trace!(target: "tcp", "{} bytes in write buffer", buf.remaining());
            if buf.remaining() < MAX_WRITE_LENGTH {
                try!(self.socket.write_all(&buf.bytes()));
                self.interest.remove(EventSet::writable());
                trace!(target: "tcp", "sent {} bytes", buf.bytes().len());
            }
            else {
                try!(self.socket.write_all(&buf.bytes()[0..MAX_WRITE_LENGTH]));
                self.buf = Some(ByteBuf::from_slice(&buf.bytes()[MAX_WRITE_LENGTH..]));
                self.interest.remove(EventSet::writable());
                trace!(target: "tcp", "sent {} bytes", MAX_WRITE_LENGTH);
            }
        }

        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
            .map_err(|e| { trace!(target: "tcp", "Error reregistering: {:?}", e); e })
    }

    fn inject_protocol_note(&self, msg: &str) -> Result<String, ()> {
        use serde_json::Value;

        let mut data: Value = try!(serde_json::from_str(msg).map_err(|_| () ));
        if data.is_object() {
            if data.as_object().unwrap().get("jsonrpc").is_none() {
                let data_mut = data.as_object_mut().unwrap();
                data_mut.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
                return Ok(try!(serde_json::to_string(&data_mut).map_err(|_| ())));
            }
        }
        Ok(msg.to_owned())
    }

    fn readable(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
        let mut buf = self.mut_buf.take().unwrap_or_else(|| panic!("unwrapping mutable buffer which is None"));

        match self.socket.try_read_buf(&mut buf) {
            Ok(None) => {
                trace!(target: "tcp", "Empty read ({:?})", self.token);
                self.mut_buf = Some(buf);
            }
            Ok(Some(_)) => {
                let (requests, last_index) = validator::extract_requests(buf.bytes());
                if requests.len() > 0 {
                    for rpc_msg in requests {
                        trace!(target: "tcp", "Request: {}", rpc_msg);

                        let channel = event_loop.channel();
                        let token = self.token.unwrap();
                        self.session.handle_request(&self.inject_protocol_note(&rpc_msg).unwrap(), move |response: Option<String>| {
                          if let Some(response_str) = response {
                            trace!(target: "tcp", "Response: {}", &response_str);

                            let mut bytes = response_str.into_bytes().to_vec();
                            bytes.extend(b"\n"); // eol

                            if let Err(e) = channel.send(RpcMessage::Write(token, bytes)) {
                              warn!(target: "ipc", "Error waking up the event loop: {:?}", e);
                            }
                          }
                        });
                    }

                    let mut new_buf = ByteBuf::mut_with_capacity(4096);
                    new_buf.write_slice(&buf.bytes()[last_index+1..]);
                    self.mut_buf = Some(new_buf);

                    self.interest.insert(EventSet::writable());
                }
                else {
                    trace!(
                        target: "tcp",
                        "Received incompelte msg ({})",
                         String::from_utf8(buf.bytes().to_vec()).unwrap_or("<non utf-8 or incomplete>".to_owned()),
                    );
                    self.mut_buf = Some(buf);
                }
            }
            Err(e) => {
                trace!(target: "tcp", "Error receiving data ({:?}): {:?}", self.token, e);
            }

        };
        self.interest.insert(EventSet::readable());
        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
            .map_err(|e| { trace!(target: "tcp", "Error reregistering: {:?}", e); e })
    }
}

struct TcpContext {
    request: RwLock<Option<RequestContext>>,
}

impl TcpContext {
    fn new() -> Arc<TcpContext> {
        Arc::new(TcpContext { request: RwLock::new(None) })
    }
}

struct RpcServer {
    socket: TcpListener,
    connections: Slab<SocketConnection>,
    io_handler: Arc<IoHandler>,
	tokens: VecDeque<Token>,
    context: Arc<TcpContext>,
    addr_index: HashMap<SocketAddr, Token>,
}

pub struct Server {
    rpc_server: Arc<RwLock<RpcServer>>,
    event_loop: Arc<RwLock<EventLoop<RpcServer>>>,
    is_stopping: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    _addr: SocketAddr,
    context: Arc<TcpContext>,
    messages: Arc<RwLock<Vec<(SocketAddr, Vec<u8>)>>>,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotStarted,
    AlreadyStopping,
    NotStopped,
    IsStopping,
}

impl std::convert::From<std::io::Error> for Error {
    fn from(io_error: std::io::Error) -> Error {
        Error::Io(io_error)
    }
}

impl Server {
    /// New server
    pub fn new(socket_addr: &SocketAddr, io_handler: &Arc<IoHandler>) -> Result<Server, Error> {
        let tcp_context = TcpContext::new();
        let (server, event_loop) = try!(RpcServer::start(socket_addr, io_handler, tcp_context.clone()));
        Ok(Server {
            rpc_server: Arc::new(RwLock::new(server)),
            event_loop: Arc::new(RwLock::new(event_loop)),
            is_stopping: Arc::new(AtomicBool::new(false)),
            is_stopped: Arc::new(AtomicBool::new(true)),
            _addr: socket_addr.clone(),
            context: tcp_context,
            messages: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Run server (in current thread)
    pub fn run(&self) {
        let mut event_loop = self.event_loop.write().unwrap();
        let mut server = self.rpc_server.write().unwrap();
        event_loop.run(&mut server).unwrap();
    }

    /// Poll server requests (for manual async scenarios)
    pub fn poll(&self) {
        let mut event_loop = self.event_loop.write().unwrap();
        let mut server = self.rpc_server.write().unwrap();

        event_loop.run_once(&mut server, Some(100)).unwrap();
    }

    /// Run server (in separate thread)
    pub fn run_async(&self) -> Result<(), Error> {
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::IsStopping) }
        if !self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStopped) }

        self.is_stopped.store(false, Ordering::Relaxed);

        let event_loop = self.event_loop.clone();
        let server = self.rpc_server.clone();
        let thread_stopping = self.is_stopping.clone();
        let thread_stopped = self.is_stopped.clone();
        let messages = self.messages.clone();
        std::thread::spawn(move || {
            let mut event_loop = event_loop.write().unwrap();
            let mut server = server.write().unwrap();
            while !thread_stopping.load(Ordering::Relaxed) {
                event_loop.run_once(&mut server, Some(POLL_INTERVAL)).unwrap();
                let mut all_messages = messages.write().unwrap();
                let total_handled =
                    if all_messages.len() > MAX_MESSAGES_DISPATCH { MAX_MESSAGES_DISPATCH }
                    else { all_messages.len() };
                if total_handled == 0 { continue; }

                trace!(target: "tcp", "Writing {} queued messages for connections", total_handled);

                let batch = all_messages.drain(0..total_handled);
                for (msg_addr, msg_data) in batch {
                    let token = server.addr_index.get(&msg_addr).and_then(|t| Some(*t));
                    match token {
                        None => {
                            trace!(target: "tcp", "{:?}: not connected to receive message", &msg_addr);
                        },
                        Some(token) => {
                            let mut connection = server.connection(token);
                            if let Err(e) = connection.send(&mut event_loop, &msg_data) {
                                trace!(target: "tcp", "{:?}: failed to send data to socket ({:?})", &msg_addr, e);
                            }
                        }
                    }
                }
            }
            thread_stopped.store(true, Ordering::Relaxed);
        });
        Ok(())
    }

    pub fn stop_async(&self) -> Result<(), Error> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        while !self.is_stopped.load(Ordering::Relaxed) { std::thread::sleep(std::time::Duration::new(0, 10)); }
        Ok(())
    }

    pub fn request_context(&self) -> Option<RequestContext> {
        self.context.request.read().unwrap().clone()
    }

    pub fn push_message(&self, socket_addr: &SocketAddr, data: &[u8]) -> io::Result<()> {
        trace!(target: "tcp", "Received payload for '{:?}' ({} bytes)", socket_addr, data.len());
        self.messages.write().unwrap().push((socket_addr.clone(), data.to_vec()));
        Ok(())
    }
}

// listener (RpcServer.socket) is never used except initializer, but have to be stored inside RpcServer and is not `Sync`
unsafe impl Sync for RpcServer { }

impl Drop for Server {
    fn drop(&mut self) {
        self.stop().unwrap_or_else(|_| {}); // ignore error - can be stopped already
    }
}

#[derive(Clone)]
pub struct RequestContext {
    pub socket_addr: SocketAddr,
}

impl RpcServer {
    /// start ipc rpc server (blocking)
    pub fn start(addr: &SocketAddr, io_handler: &Arc<IoHandler>, tcp_context: Arc<TcpContext>) -> Result<(RpcServer, EventLoop<RpcServer>), Error> {
        let mut event_loop = try!(EventLoop::new());
        let socket = try!(TcpListener::bind(&addr));
        event_loop.register(&socket, SERVER, EventSet::readable(), PollOpt::edge()).unwrap();
        let server = RpcServer {
            socket: socket,
            connections: Slab::new_starting_at(Token(1), MAX_CONCURRENT_CONNECTIONS),
            io_handler: io_handler.clone(),
			tokens: VecDeque::new(),
            context: tcp_context,
            addr_index: HashMap::new(),
        };
        Ok((server, event_loop))
    }

    fn accept(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
        let (stream, addr) = self.socket.accept().unwrap().unwrap();
        let connection = SocketConnection::new(stream, addr.clone(), self.io_handler.session());
        if self.connections.count() >= MAX_CONCURRENT_CONNECTIONS {
            // max connections
            return Ok(());
        }
        let token = self.connections.insert(connection).ok().expect("fatal: Could not add connection to slab (memory issue?)");
		self.tokens.push_back(token);

        self.addr_index.insert(addr.clone(), token);

		trace!(target: "tcp", "Accepted connection with token {:?}, socket address: {:?}", token, addr);

        self.connections[token].token = Some(token);
        event_loop.register(
            &self.connections[token].socket,
            token,
            EventSet::readable(),
            PollOpt::edge() | PollOpt::oneshot()
        ).ok().expect("fatal: could not register socket with event loop (memory issue?)");

        Ok(())
    }

    fn connection_send(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token, data: Vec<u8>) -> io::Result<()> {
        self.connection(tok).send(event_loop, &data)
    }

    fn connection_readable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        *self.context.request.write().unwrap() =
            Some(RequestContext { socket_addr: self.connection(tok).addr.clone() });
        self.connection(tok).readable(event_loop)
    }

    fn connection_writable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        self.connection(tok).writable(event_loop)
    }

    fn connection<'a>(&'a mut self, tok: Token) -> &'a mut SocketConnection {
        &mut self.connections[tok]
    }

    fn drop_connection(&mut self, tok: Token) {
        trace!(target: "tcp", "Dropping connection {:?}", tok);
        self.connections.remove(tok);
    }
}

enum RpcMessage {
  Write(Token, Vec<u8>) ,
}


impl Handler for RpcServer {
    type Timeout = usize;
    type Message = RpcMessage;

    fn notify(&mut self, event_loop: &mut EventLoop<RpcServer>, msg: RpcMessage) {
      match msg {
        RpcMessage::Write(token, bytes) => self.connection_send(event_loop, token, bytes).unwrap(),
      }
    }

    fn ready(&mut self, event_loop: &mut EventLoop<RpcServer>, token: Token, events: EventSet) {
      if events.is_readable() {
            match token {
                SERVER => self.accept(event_loop).unwrap(),
                _ => self.connection_readable(event_loop, token).unwrap()
            };
        }

        if events.is_hup() {
            match token {
                SERVER => { trace!(target: "tcp", "Server hup"); },
                other_token => {
                    self.drop_connection(other_token)
                }
            }
        }

        if events.is_writable() {
            match token {
                SERVER => { },
                _ => self.connection_writable(event_loop, token).unwrap_or_else(|_| {}), // todo: disqualify connection from list on error
            };
        }
    }
}
