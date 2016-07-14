//! jsonrpc http server.
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
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
//! 	let _server = ServerBuilder::new(Arc::new(io)).start_http(&"127.0.0.1:3030".parse().unwrap());
//! }
//! ```

extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;

mod cors;
mod request_response;
mod handler;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::thread;
use hyper::server;
use jsonrpc::IoHandler;

pub use hyper::header::AccessControlAllowOrigin;
pub use handler::{PanicHandler, ServerHandler};

pub type ServerResult = Result<Server, RpcServerError>;

/// RPC Server startup error
#[derive(Debug)]
pub enum RpcServerError {
	IoError(std::io::Error),
	Other(hyper::error::Error),
}

impl From<hyper::error::Error> for RpcServerError {
	fn from(err: hyper::error::Error) -> Self {
		match err {
			hyper::error::Error::Io(e) => RpcServerError::IoError(e),
			e => RpcServerError::Other(e)
		}
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder {
	jsonrpc_handler: Arc<IoHandler>,
	cors: Vec<AccessControlAllowOrigin>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
}

impl ServerBuilder {
	pub fn new(jsonrpc_handler: Arc<IoHandler>) -> Self {
		ServerBuilder {
			jsonrpc_handler: jsonrpc_handler,
			cors: Vec::new(),
			panic_handler: None,
		}
	}

	pub fn panic_handler<F>(mut self, handler: F) -> Self where F : Fn() -> () + Send + 'static {
		self.panic_handler = Some(Box::new(handler));
		self
	}

	pub fn cors_domains(mut self, cors: Vec<AccessControlAllowOrigin>) -> Self {
		self.cors = cors;
		self
	}

	pub fn start_http(self, addr: &SocketAddr) -> ServerResult {
		let panic_for_server = Arc::new(Mutex::new(self.panic_handler));
		let jsonrpc_handler = self.jsonrpc_handler;
		let cors_domains = self.cors;

		let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |_| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), handler)
		}));

		thread::spawn(move || {
			srv.run();
		});

		Ok(Server {
			server: Some(l),
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	server: Option<server::Listening>,
}

impl Server {
	pub fn addr(&self) -> &SocketAddr {
		self.server.as_ref().unwrap().addr()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}
