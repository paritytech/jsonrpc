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
//! extern crate jsonrpc_ipc_server;
//! extern crate rand;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_ipc_server::Server;
//!
//! fn main() {
//! 	let mut io = IoHandler::default();
//! 	io.add_method("say_hello", |_params| {
//!			Ok(Value::String("hello".into()))
//! 	});
//!
//! 	let server = Server::new("/tmp/json-ipc-test.ipc", io).unwrap();
//!     ::std::thread::spawn(move || server.run());
//! }
//! ```

use std;
use std::io::{self, Write};
use std::collections::VecDeque;
use std::sync::*;
use std::sync::atomic::*;

use bytes::{ByteBuf, MutByteBuf};
use jsonrpc_core::MetaIoHandler;
use jsonrpc_core::reactor::{RpcHandler, RpcEventLoop, RpcEventLoopHandle};
use mio::*;
use mio::unix::*;

use slab;
use validator;
#[cfg(test)]
use tests;

type Bytes = Vec<u8>;

const SERVER: Token = Token(0);
const MAX_CONCURRENT_CONNECTIONS: usize = 1024;
const MAX_WRITE_LENGTH: usize = 8192;
const REQUEST_CHUNK_SIZE: usize = 4096;

struct SocketConnection {
	socket: UnixStream,
	session: RpcHandler,
	write_buf: Option<Bytes>,
	read_buf: MutByteBuf,
	token: Option<Token>,
	interest: EventSet,
	request: Bytes,
}

type Slab<T> = slab::Slab<T, Token>;

impl SocketConnection {
	fn new(sock: UnixStream, session: RpcHandler) -> Self {
		SocketConnection {
			socket: sock,
			session: session,
			write_buf: None,
			read_buf: ByteBuf::mut_with_capacity(REQUEST_CHUNK_SIZE),
			token: None,
			interest: EventSet::hup(),
			request: Vec::with_capacity(REQUEST_CHUNK_SIZE),
		}
	}

	fn write(&mut self, event_loop: &mut EventLoop<RpcServer>, data: Bytes) -> io::Result<()> {
		let bytes = {
			if let Some(mut bytes) = self.write_buf.take() {
				bytes.extend_from_slice(&data);
				bytes
			} else {
				self.interest.insert(EventSet::writable());
				data
			}
		};
		self.write_buf = Some(bytes);

		event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
	}

	fn writable(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
		if let Some(buf) = self.write_buf.take() {
			if buf.len() < MAX_WRITE_LENGTH {
				try!(self.socket.write_all(&buf));
				self.interest.remove(EventSet::writable());
				self.interest.insert(EventSet::readable());
			} else {
				try!(self.socket.write_all(&buf[0..MAX_WRITE_LENGTH]));
				self.write_buf = Some(buf[MAX_WRITE_LENGTH..].to_vec());
			}
		} else {
			self.interest.remove(EventSet::writable());
		}

		event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
	}

	fn readable(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
		match self.socket.try_read_buf(&mut self.read_buf) {
			Ok(None) => {
				trace!(target: "ipc", "Empty read ({:?})", self.token);
			}
			Ok(Some(_)) => {
				self.request.extend(self.read_buf.bytes());
				let (requests, last_index) = validator::extract_requests(&self.request);
				if !requests.is_empty() {
					for rpc_msg in requests {
						trace!(target: "ipc", "Request: {}", rpc_msg);
						let channel = event_loop.channel();
						let token = self.token.unwrap();
						let metadata = ();

						self.session.handle_request(&rpc_msg, metadata, move |response: Option<String>| {
							if let Some(response_str) = response {
								trace!(target: "ipc", "Response: {}", &response_str);
								if let Err(e) = channel.send(RpcMessage::Write(token, response_str.into_bytes())) {
 									warn!(target: "ipc", "Error waking up the event loop: {:?}", e);
 								}
							}
						});
					}

					let left_over = self.request.drain(last_index + 1..).collect::<Bytes>();
					self.request = Vec::with_capacity(REQUEST_CHUNK_SIZE);
					self.request.extend(&left_over);

					self.interest.remove(EventSet::readable());
					self.interest.insert(EventSet::writable());
				} else {
					self.interest.insert(EventSet::readable());
					trace!(target: "ipc", "Incomplete request: {}", String::from_utf8(self.request.clone()).unwrap_or("<non-utf>".to_owned()));
				}
				self.read_buf.clear();
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
	socket: UnixListener,
	connections: Slab<SocketConnection>,
	io_handler: RpcHandler,
	tokens: VecDeque<Token>,
}

pub struct Server {
	rpc_server: Arc<RwLock<RpcServer>>,
	event_loop: Arc<RwLock<EventLoop<RpcServer>>>,
	is_stopping: Arc<AtomicBool>,
	is_stopped: Arc<AtomicBool>,
	rpc_event_loop: Mutex<Option<RpcEventLoopHandle>>,
	addr: String,
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
	pub fn new<T>(socket_addr: &str, io_handler: T) -> Result<Server, Error> where
		T: Into<MetaIoHandler<()>>
	{
		let rpc_loop = RpcEventLoop::spawn(Arc::new(io_handler.into()));
		let mut server = try!(Self::with_rpc_handler(socket_addr, rpc_loop.handler()));
		server.rpc_event_loop = Mutex::new(Some(rpc_loop.into()));
		Ok(server)
	}

	pub fn with_rpc_handler(socket_addr: &str, io_handler: RpcHandler) -> Result<Server, Error> {
		let (server, event_loop) = try!(RpcServer::start(socket_addr, io_handler));

		Ok(Server {
			rpc_server: Arc::new(RwLock::new(server)),
			event_loop: Arc::new(RwLock::new(event_loop)),
			is_stopping: Arc::new(AtomicBool::new(false)),
			is_stopped: Arc::new(AtomicBool::new(true)),
			rpc_event_loop: Mutex::new(None),
			addr: socket_addr.to_owned(),
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
		if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping) }
		self.is_stopping.store(true, Ordering::Relaxed);
		Ok(())
	}

	pub fn stop(&self) -> Result<(), Error> {
		if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
		if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping) }
		self.is_stopping.store(true, Ordering::Relaxed);
		while !self.is_stopped.load(Ordering::Relaxed) { std::thread::sleep(std::time::Duration::new(0, 10)); }
		Ok(())
	}
}


impl Drop for Server {
	fn drop(&mut self) {
		self.stop().unwrap_or_else(|_| {}); // ignore error - can be stopped already
		::std::fs::remove_file(&self.addr).unwrap_or_else(|_| {}); // ignoer error - server could have never been started
	}
}

impl RpcServer {
	/// start ipc rpc server
	pub fn start(addr: &str, io_handler: RpcHandler) -> Result<(RpcServer, EventLoop<RpcServer>), Error> {
		let mut event_loop = try!(EventLoop::new());
		::std::fs::remove_file(addr).unwrap_or_else(|_| {}); // ignore error (if no file)
		let socket = try!(UnixListener::bind(&addr));
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
		let new_client_socket = self.socket.accept().unwrap().unwrap();
		let connection = SocketConnection::new(new_client_socket, self.io_handler.clone());
		if self.connections.count() >= MAX_CONCURRENT_CONNECTIONS {
			// max connections
			return Ok(());
		}
		let token = self.connections.insert(connection).ok().expect("fatal: Could not add connection to slab (memory issue?)");
		self.tokens.push_back(token);

		trace!(target: "ipc", "Accepted connection with token {:?}", token);

		self.connections[token].token = Some(token);
		event_loop.register(
			&self.connections[token].socket,
			token,
			EventSet::readable(),
			PollOpt::edge() | PollOpt::oneshot()
		).ok().expect("fatal: could not register socket with event loop (memory issue?)");

		Ok(())
	}

	fn connection_write(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token, data: Bytes) -> io::Result<()> {
		// Ignore notifications for disconnected sockets.
		if self.connections.contains(tok) {
			self.connection(tok).write(event_loop, data)
		} else {
			Ok(())
		}
	}

	fn connection_readable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
		self.connection(tok).readable(event_loop)
	}

	fn connection_writable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
		self.connection(tok).writable(event_loop)
	}

	fn connection<'a>(&'a mut self, tok: Token) -> &'a mut SocketConnection {
		&mut self.connections[tok]
	}

	fn drop_connection(&mut self, tok: Token) {
		trace!(target: "ipc", "Dropping connection {:?}", tok);
		self.connections.remove(tok);
	}
}

enum RpcMessage {
	Write(Token, Bytes)
}

impl Handler for RpcServer {
	type Timeout = usize;
	type Message = RpcMessage;

	fn notify(&mut self, event_loop: &mut EventLoop<RpcServer>, msg: RpcMessage) {
		match msg {
			RpcMessage::Write(token, bytes) => self.connection_write(event_loop, token, bytes).unwrap()
		}
	}

	fn ready(&mut self, event_loop: &mut EventLoop<RpcServer>, token: Token, events: EventSet) {
		if events.is_readable() {
			match token {
				SERVER => self.accept(event_loop).unwrap(),
				_ => self.connection_readable(event_loop, token).unwrap()
			};
		}

		if events.is_writable() {
			match token {
				SERVER => {},
				_ => self.connection_writable(event_loop, token).unwrap_or_else(|_| {}), // todo: disqualify connection from list on error
			};
		}

		if events.is_hup() {
			match token {
				SERVER => {
					trace!(target: "ipc", "Server hup");
				},
				other_token => {
					self.drop_connection(other_token)
				}
			}
		}
	}
}

#[test]
pub fn test_reqrep_poll() {
	let addr = tests::random_ipc_endpoint();
	let io = tests::dummy_io_handler();
	let server = Server::new(&addr, io).unwrap();
	std::thread::spawn(move || {
		loop {
			server.poll();
		}
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
	assert_eq!(String::from_utf8(tests::dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

	std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
pub fn test_file_removed() {
	let addr = tests::random_ipc_endpoint();
	let io = tests::dummy_io_handler();
	{
		let server = Server::new(&addr, io).unwrap();
		server.run_async().unwrap();
		std::thread::sleep(std::time::Duration::from_millis(50));

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
		assert_eq!(String::from_utf8(tests::dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
	}
	assert!(::std::fs::metadata(addr).is_err()); // err is file not exists
}
