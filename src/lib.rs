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
//! 	let _server = Server::start(&"127.0.0.1:3030".parse().unwrap(), Arc::new(io), vec![AccessControlAllowOrigin::Null]);
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

/// jsonrpc http server.
pub struct Server {
	server: Option<server::Listening>,
	panic_handler: Arc<Mutex<Option<Box<Fn() -> () + Send>>>>
}

impl Server {
	pub fn start(addr: &SocketAddr, jsonrpc_handler: Arc<IoHandler>, cors_domains: Vec<AccessControlAllowOrigin>) -> ServerResult {
		let panic_handler = Arc::new(Mutex::new(None));
		let panic_for_server = panic_handler.clone();
		let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |_| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), handler)
		}));

		thread::spawn(move || {
			srv.run();
		});

		Ok(Server {
			server: Some(l),
			panic_handler: panic_handler,
		})
	}

	pub fn set_panic_handler<F>(&self, handler: F) where F : Fn() -> () + Send + 'static {
		*self.panic_handler.lock().unwrap() = Some(Box::new(handler));
	}

	pub fn addr(&self) -> &SocketAddr {
		self.server.as_ref().unwrap().addr()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}
