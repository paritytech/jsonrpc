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

use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::thread;
use std::collections::HashSet;
use std::ops::Deref;
use hyper::server;
use jsonrpc::MetaIoHandler;
use jsonrpc::reactor::{RpcHandler, RpcEventLoop, RpcEventLoopHandle};

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

/// Extracts metadata from the HTTP request.
pub trait HttpMetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &server::Request<hyper::net::HttpStream>) -> M {
		Default::default()
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata> HttpMetaExtractor<M> for NoopExtractor {}

/// Basic RPC abstraction.
#[derive(Clone)]
pub struct Rpc<M: jsonrpc::Metadata> {
	/// RPC Handler
	pub handler: RpcHandler<M>,
	/// Metadata extractor
	pub extractor: Arc<HttpMetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata> Deref for Rpc<M> {
	type Target = RpcHandler<M>;

	fn deref(&self) -> &Self::Target {
		&self.handler
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = ()> {
	jsonrpc_handler: Arc<MetaIoHandler<M>>,
	remote: Option<tokio_core::reactor::Remote>,
	meta_extractor: Arc<HttpMetaExtractor<M>>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
}

impl<M: jsonrpc::Metadata> ServerBuilder<M> {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// If you want to re-use the same handler in couple places
	/// see `with_remote` function.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self where
		T: Into<MetaIoHandler<M>>
	{
		ServerBuilder {
			jsonrpc_handler: Arc::new(handler.into()),
			remote: None,
			meta_extractor: Arc::new(NoopExtractor::default()),
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
	pub fn with_remote(handler: Arc<MetaIoHandler<M>>, remote: tokio_core::reactor::Remote) -> Self {
		ServerBuilder {
			jsonrpc_handler: handler,
			remote: Some(remote),
			meta_extractor: Arc::new(NoopExtractor::default()),
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

		let (handler, event_loop) = match self.remote {
			Some(remote) => (RpcHandler::new(self.jsonrpc_handler, remote), None),
			None => {
				let event_loop = RpcEventLoop::spawn(self.jsonrpc_handler);
				(event_loop.handler(), Some(event_loop.into()))
			},
		};

		let jsonrpc_handler = Rpc {
			handler: handler,
			extractor: self.meta_extractor,
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
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	server: Option<server::Listening>,
	handle: Option<thread::JoinHandle<()>>,
	event_loop: Option<RpcEventLoopHandle>,
}

impl Server {
	/// Returns addresses of this server
	pub fn addrs(&self) -> &[SocketAddr] {
		self.server.as_ref().unwrap().addrs()
	}

	/// Closes the server.
	pub fn close(mut self) {
		self.server.take().unwrap().close();
		self.event_loop.take().map(|v| v.close());
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close();
		self.event_loop.take().map(|v| v.close());
	}
}
