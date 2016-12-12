//! jsonrpc http server.
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//!
//! fn main() {
//! 	let mut io = IoHandler::default();
//! 	io.add_method("say_hello", |_: Params| {
//! 		Ok(Value::String("hello".to_string()))
//! 	});
//!
//! 	let _server = ServerBuilder::new(io).start_http(&"127.0.0.1:3030".parse().unwrap());
//! }
//! ```

#![warn(missing_docs)]

#[macro_use] extern crate log;
extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;
extern crate futures;
extern crate tokio_core;

pub mod request_response;
pub mod cors;
mod handler;
mod hosts_validator;
#[cfg(test)]
mod tests;

use std::sync::{mpsc, Arc, Mutex};
use std::net::SocketAddr;
use std::thread;
use std::collections::HashSet;
use futures::Future;
use hyper::server;
use jsonrpc::MetaIoHandler;

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

impl From<std::io::Error> for RpcServerError {
	fn from(err: std::io::Error) -> Self {
		RpcServerError::IoError(err)
	}
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

/// Basic RPC abstraction.
#[derive(Clone)]
pub struct RpcHandler {
	/// RPC Handler
	pub handler: Arc<MetaIoHandler<()>>,
	/// Event Loop Remote
	pub remote: tokio_core::reactor::Remote,
}

impl RpcHandler {
	/// Handles the request and returns to a closure response when it's ready.
	pub fn handle_request<F>(&self, request: &str, on_response: F) where
		F: Fn(Option<String>) + Send + 'static
	{
		let future = self.handler.handle_request(request, ());
		self.remote.spawn(|_| future.map(on_response))
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder {
	jsonrpc_handler: Arc<MetaIoHandler<()>>,
	remote: Option<tokio_core::reactor::Remote>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
}

impl ServerBuilder {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// If you want to re-use the same handler in couple places
	/// see `with_remote` function.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self where
		T: Into<MetaIoHandler<()>>
	{
		ServerBuilder {
			jsonrpc_handler: Arc::new(handler.into()),
			remote: None,
			cors_domains: None,
			allowed_hosts: None,
			panic_handler: None,
		}
	}

	/// Creates new `ServerBuilder` given access to the event loop `Remote`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn with_remote(handler: Arc<MetaIoHandler<()>>, remote: tokio_core::reactor::Remote) -> Self {
		ServerBuilder {
			jsonrpc_handler: handler,
			remote: Some(remote),
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
		let cors_domains = self.cors_domains;
		let hosts = Arc::new(Mutex::new(self.allowed_hosts));
		let hosts_setter = hosts.clone();

		let (remote, event_loop, event_loop_handle) = match self.remote {
			Some(remote) => (remote, None, None),
			None => {
				let (stop, stopped) = futures::oneshot();
				let (tx, rx) = mpsc::channel();
				let handle = thread::spawn(move || {
					let mut el = tokio_core::reactor::Core::new().expect("Creating an event loop should not fail.");
					tx.send(el.remote()).expect("Rx is blocking upper thread.");
					let _ = el.run(futures::empty().select(stopped));
				});
				let remote = rx.recv().expect("tx is transfered to a newly spawned thread.");
				(remote, Some(stop), Some(handle))
			},
		};

		let jsonrpc_handler = RpcHandler {
			handler: self.jsonrpc_handler,
			remote: remote,
		};

		let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |control| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			let hosts = hosts.lock().unwrap().clone();
			ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), hosts, handler, control)
		}));

		// Add current host to allowed headers.
		// NOTE: we need to use `l.addrs()` instead of `addr`
		// it might be different!
		{
			let mut hosts = hosts_setter.lock().unwrap();
			if let Some(current_hosts) = hosts.take() {
				let mut new_hosts = current_hosts.into_iter().collect::<HashSet<_>>();
				for addr in l.addrs() {
					let address = addr.to_string();
					new_hosts.insert(address.clone());
					new_hosts.insert(address.replace("127.0.0.1", "localhost"));
				}
				// Override hosts
				*hosts = Some(new_hosts.into_iter().collect());
			}
		}

		let handle = thread::spawn(move || {
			srv.run();
		});

		Ok(Server {
			server: Some(l),
			handle: Some(handle),
			event_loop: event_loop,
			event_loop_handle: event_loop_handle,
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	server: Option<server::Listening>,
	handle: Option<thread::JoinHandle<()>>,
	event_loop: Option<futures::Complete<()>>,
	event_loop_handle: Option<thread::JoinHandle<()>>
}

impl Server {
	/// Returns addresses of this server
	pub fn addrs(&self) -> &[SocketAddr] {
		self.server.as_ref().unwrap().addrs()
	}

	/// Closes the server.
	pub fn close(mut self) {
		self.server.take().unwrap().close();
		self.event_loop.take().map(|v| v.complete(()));
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		try!(self.handle.take().unwrap().join());
		self.event_loop_handle.take().map(|v| v.join()).unwrap_or(Ok(()))
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close();
		self.event_loop.take().map(|v| v.complete(()));
	}
}
