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
//! impl SyncMethodCommand for SayHello {
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

#![warn(missing_docs)]

#[macro_use] extern crate log;
extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;

pub mod request_response;
mod cors;
mod handler;
mod hosts_validator;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::thread;
use hyper::server;
use jsonrpc::IoHandler;

pub use hyper::header::AccessControlAllowOrigin;
pub use handler::{PanicHandler, ServerHandler};
pub use hosts_validator::is_host_header_valid;

/// Result of starting the Server.
pub type ServerResult = Result<Server, RpcServerError>;

/// RPC Server startup error.
#[derive(Debug)]
pub enum RpcServerError {
	/// IO Error
	IoError(std::io::Error),
	/// Other Error (hyper)
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

/// Specifies if domains should be validated.
pub enum DomainsValidation<T> {
	/// Allow only domains on the list.
	AllowOnly(Vec<T>),
	/// Disable domains validation completely.
	Disabled,
}

impl<T> Into<Option<Vec<T>>> for DomainsValidation<T> {
	fn into(self) -> Option<Vec<T>> {
		use DomainsValidation::*;
		match self {
			AllowOnly(list) => Some(list),
			Disabled => None,
		}
	}
}

impl<T> From<Option<Vec<T>>> for DomainsValidation<T> {
	fn from(other: Option<Vec<T>>) -> Self {
		match other {
			Some(list) => DomainsValidation::AllowOnly(list),
			None => DomainsValidation::Disabled,
		}
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder {
	jsonrpc_handler: Arc<IoHandler>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
}

impl ServerBuilder {
	/// Creates new `ServerBuilder` with specified `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new(jsonrpc_handler: Arc<IoHandler>) -> Self {
		ServerBuilder {
			jsonrpc_handler: jsonrpc_handler,
			cors_domains: None,
			allowed_hosts: None,
			panic_handler: None,
		}
	}

	/// Sets handler invoked in case of server panic.
	pub fn panic_handler<F>(mut self, handler: F) -> Self where F : Fn() -> () + Send + 'static {
		self.panic_handler = Some(Box::new(handler));
		self
	}

	/// Configures a list of allowed CORS origins.
	pub fn cors(mut self, cors_domains: DomainsValidation<AccessControlAllowOrigin>) -> Self {
		self.cors_domains = cors_domains.into();
		self
	}

	/// Allow connections only with `Host` header set to binding address.
	pub fn allow_only_bind_host(mut self) -> Self {
		self.allowed_hosts = Some(Vec::new());
		self
	}

	/// Specify a list of valid `Host` headers. Binding address is allowed automatically.
	pub fn allowed_hosts(mut self, allowed_hosts: DomainsValidation<String>) -> Self {
		self.allowed_hosts = allowed_hosts.into();
		self
	}

	/// Start this JSON-RPC HTTP server trying to bind to specified `SocketAddr`.
	pub fn start_http(self, addr: &SocketAddr) -> ServerResult {
		let panic_for_server = Arc::new(Mutex::new(self.panic_handler));
		let jsonrpc_handler = self.jsonrpc_handler;
		let cors_domains = self.cors_domains;
		let hosts = Arc::new(Mutex::new(self.allowed_hosts));
		let hosts_setter = hosts.clone();

		let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |control| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			let hosts = hosts.lock().unwrap().clone();
			ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), hosts, handler, control)
		}));

		// Add current host to allowed headers.
		// NOTE: we need to use `l.addr()` instead of `addr`
		// it might be different!
		{
			let mut hosts = hosts_setter.lock().unwrap();
			if let Some(ref mut hosts) = *hosts {
				let address = l.addr().to_string();
				hosts.push(address.clone());
				hosts.push(address.replace("127.0.0.1", "localhost"));
			}
		}

		let handle = thread::spawn(move || {
			srv.run();
		});

		Ok(Server {
			server: Some(l),
			handle: Some(handle),
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	server: Option<server::Listening>,
	handle: Option<thread::JoinHandle<()>>,
}

impl Server {
	/// Returns address of this server
	pub fn addr(&self) -> &SocketAddr {
		self.server.as_ref().unwrap().addr()
	}

	/// Closes the server.
	pub fn close(mut self) {
		self.server.take().unwrap().close()
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}
