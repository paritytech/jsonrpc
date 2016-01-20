//! jsonrpc http server.
//! 
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//! 
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//! 
//! struct SayHello;
//! impl MethodCommand for SayHello {
//! 	fn execute(&mut self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//! 
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let server = Server::new(io, 1);
//! 	server.start("127.0.0.1:3030");
//! }
//! ```

extern crate hyper;
extern crate jsonrpc_core as jsonrpc;

use std::thread;
use std::sync::Mutex;
use std::io::Read;
use self::jsonrpc::{IoHandler};

/// jsonrpc http request handler.
struct ServerHandler {
	jsonrpc_handler: Mutex<IoHandler>
}

impl ServerHandler {
	/// Create new request handler.
	fn new(jsonrpc_handler: IoHandler) -> Self {
		ServerHandler {
			jsonrpc_handler: Mutex::new(jsonrpc_handler)
		}
	}
}

impl hyper::server::Handler for ServerHandler {
	fn handle(&self, mut req: hyper::server::Request, mut res: hyper::server::Response) {
		match req.method {
			hyper::Post => { 
				let mut body = String::new();
				if let Err(_) = req.read_to_string(&mut body) {
					// TODO: return proper jsonrpc error instead
					*res.status_mut() = hyper::status::StatusCode::MethodNotAllowed;
					return;
				}
				if let Some(response) = self.jsonrpc_handler.lock().unwrap().handle_request(&body) {
					res.send(response.as_ref()).unwrap();
				}
			},
			_ => *res.status_mut() = hyper::status::StatusCode::MethodNotAllowed
		}
	}
}

/// jsonrpc http server.
/// 
/// ```no_run
/// extern crate jsonrpc_core;
/// extern crate jsonrpc_http_server;
/// 
/// use jsonrpc_core::*;
/// use jsonrpc_http_server::*;
/// 
/// struct SayHello;
/// impl MethodCommand for SayHello {
/// 	fn execute(&mut self, _params: Params) -> Result<Value, Error> {
/// 		Ok(Value::String("hello".to_string()))
/// 	}
/// }
/// 
/// fn main() {
/// 	let mut io = IoHandler::new();
/// 	io.add_method("say_hello", SayHello);
/// 	let server = Server::new(io, 1);
/// 	server.start("127.0.0.1:3030");
/// }
/// ```
pub struct Server {
	jsonrpc_handler: IoHandler,
	threads: usize
}

impl Server {
	pub fn new(jsonrpc_handler: IoHandler, threads: usize) -> Self { 
		Server {
			jsonrpc_handler: jsonrpc_handler,
			threads: threads
		}
	}

	pub fn start(self, addr: &str) {
		hyper::Server::http(addr).unwrap().handle_threads(ServerHandler::new(self.jsonrpc_handler), self.threads).unwrap();
	}

	pub fn start_async(self, addr: &str) {
		let address = addr.to_owned();
		thread::Builder::new().name("jsonrpc_http".to_string()).spawn(move || {
			self.start(address.as_ref());
		}).unwrap();
	}
}

