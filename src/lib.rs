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
//! impl MethodCommand for SayHello {
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

#[cfg(test)]
extern crate rand;

mod validator;
#[cfg(test)]
pub mod tests;

use mio::*;
use mio::tcp::*;
use bytes::{Buf, ByteBuf, MutByteBuf};
use std::io;
use jsonrpc_core::IoHandler;
use std::sync::*;
use std::sync::atomic::*;
use std::collections::VecDeque;
use std::env;
use log::LogLevelFilter;
use env_logger::LogBuilder;
use std::net::SocketAddr;

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

struct SocketConnection {
    socket: TcpStream,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>,
    token: Option<Token>,
    interest: EventSet,
    addr: SocketAddr,
}

type Slab<T> = slab::Slab<T, Token>;

impl SocketConnection {
    fn new(sock: TcpStream, addr: SocketAddr) -> Self {
        SocketConnection {
            socket: sock,
            buf: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(4096)),
            token: None,
            interest: EventSet::hup(),
            addr: addr,
        }
    }

    fn writable(&mut self, event_loop: &mut EventLoop<RpcServer>, _handler: &IoHandler) -> io::Result<()> {
        use std::io::Write;
        if let Some(buf) = self.buf.take() {
            if buf.remaining() < MAX_WRITE_LENGTH {
                try!(self.socket.write_all(&buf.bytes()));
                self.interest.remove(EventSet::writable());
                self.interest.insert(EventSet::readable());
            }
            else {
                try!(self.socket.write_all(&buf.bytes()[0..MAX_WRITE_LENGTH]));
                self.buf = Some(ByteBuf::from_slice(&buf.bytes()[MAX_WRITE_LENGTH..]));
            }
        }

        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }

    fn readable(&mut self, event_loop: &mut EventLoop<RpcServer>, handler: &IoHandler) -> io::Result<()> {
        let mut buf = self.mut_buf.take().unwrap_or_else(|| panic!("unwrapping mutable buffer which is None"));

        match self.socket.try_read_buf(&mut buf) {
            Ok(None) => {
                trace!(target: "ipc", "Empty read ({:?})", self.token);
                self.mut_buf = Some(buf);
            }
            Ok(Some(_)) => {
                let (requests, last_index) = validator::extract_requests(buf.bytes());
                if requests.len() > 0 {
                    let mut response_bytes = Vec::new();
                    for rpc_msg in requests {
                        trace!(target: "ipc", "Request: {}", rpc_msg);
                        let response: Option<String> = handler.handle_request(&rpc_msg);
                        if let Some(response_str) = response {
                            trace!(target: "ipc", "Response: {}", &response_str);
                            response_bytes.extend(response_str.into_bytes());
                        }
                    }
                    self.buf = Some(ByteBuf::from_slice(&response_bytes[..]));

                    let mut new_buf = ByteBuf::mut_with_capacity(4096);
                    new_buf.write_slice(&buf.bytes()[last_index+1..]);
                    self.mut_buf = Some(new_buf);

                    self.interest.remove(EventSet::readable());
                    self.interest.insert(EventSet::writable());
                }
                else {
                    self.mut_buf = Some(buf);
                }
            }
            Err(e) => {
                trace!(target: "ipc", "Error receiving data ({:?}): {:?}", self.token, e);
                self.interest.remove(EventSet::readable());
            }

        };
        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }
}

struct RpcServer {
    socket: TcpListener,
    connections: Slab<SocketConnection>,
    io_handler: Arc<IoHandler>,
	tokens: VecDeque<Token>,
}

pub struct Server {
    rpc_server: Arc<RwLock<RpcServer>>,
    event_loop: Arc<RwLock<EventLoop<RpcServer>>>,
    is_stopping: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    addr: SocketAddr,
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
        let (server, event_loop) = try!(RpcServer::start(socket_addr, io_handler));
        Ok(Server {
            rpc_server: Arc::new(RwLock::new(server)),
            event_loop: Arc::new(RwLock::new(event_loop)),
            is_stopping: Arc::new(AtomicBool::new(false)),
            is_stopped: Arc::new(AtomicBool::new(true)),
            addr: socket_addr.clone(),
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
        std::thread::spawn(move || {
            let mut event_loop = event_loop.write().unwrap();
            let mut server = server.write().unwrap();
            while !thread_stopping.load(Ordering::Relaxed) {
                event_loop.run_once(&mut server, Some(100)).unwrap();
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
}

// listener (RpcServer.socket) is never used except initializer, but have to be stored inside RpcServer and is not `Sync`
unsafe impl Sync for RpcServer { }

impl Drop for Server {
    fn drop(&mut self) {
        self.stop().unwrap_or_else(|_| {}); // ignore error - can be stopped already
    }
}

impl RpcServer {
    /// start ipc rpc server (blocking)
    pub fn start(addr: &SocketAddr, io_handler: &Arc<IoHandler>) -> Result<(RpcServer, EventLoop<RpcServer>), Error> {
        let mut event_loop = try!(EventLoop::new());
        let socket = try!(TcpListener::bind(&addr));
        event_loop.register(&socket, SERVER, EventSet::readable(), PollOpt::edge()).unwrap();
        let server = RpcServer {
            socket: socket,
            connections: Slab::new_starting_at(Token(1), MAX_CONCURRENT_CONNECTIONS),
            io_handler: io_handler.clone(),
			tokens: VecDeque::new(),
        };
        Ok((server, event_loop))
    }

    fn accept(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
        let (stream, addr) = self.socket.accept().unwrap().unwrap();
        let connection = SocketConnection::new(stream, addr.clone());
        if self.connections.count() >= MAX_CONCURRENT_CONNECTIONS {
            // max connections
            return Ok(());
        }
        let token = self.connections.insert(connection).ok().expect("fatal: Could not add connection to slab (memory issue?)");
		self.tokens.push_back(token);

		trace!(target: "ipc", "Accepted connection with token {:?}, socket address: {:?}", token, addr);

        self.connections[token].token = Some(token);
        event_loop.register(
            &self.connections[token].socket,
            token,
            EventSet::readable(),
            PollOpt::edge() | PollOpt::oneshot()
        ).ok().expect("fatal: could not register socket with event loop (memory issue?)");

        Ok(())
    }

    fn connection_readable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        let io_handler = self.io_handler.clone();
        self.connection(tok).readable(event_loop, &io_handler)
    }

    fn connection_writable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        let io_handler = self.io_handler.clone();
        self.connection(tok).writable(event_loop, &io_handler)
    }

    fn connection<'a>(&'a mut self, tok: Token) -> &'a mut SocketConnection {
        &mut self.connections[tok]
    }

    fn drop_connection(&mut self, tok: Token) {
        trace!(target: "ipc", "Dropping connection {:?}", tok);
        self.connections.remove(tok);
    }
}


impl Handler for RpcServer {
    type Timeout = usize;
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<RpcServer>, token: Token, events: EventSet) {
        if events.is_readable() {
            match token {
                SERVER => self.accept(event_loop).unwrap(),
                _ => self.connection_readable(event_loop, token).unwrap()
            };
        }

        if events.is_hup() {
            match token {
                SERVER => { trace!(target: "ipc", "Server hup"); },
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
