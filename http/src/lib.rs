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
//! 	let mut io = IoHandler::new();
//! 	io.add_method("say_hello", |_: Params| {
//! 		Ok(Value::String("hello".to_string()))
//! 	});
//!
//! 	let _server = ServerBuilder::new(io).start_http(&"127.0.0.1:3030".parse().unwrap());
//! }
//! ```

#![warn(missing_docs)]

#[macro_use] extern crate log;
extern crate unicase;
extern crate parking_lot;
extern crate jsonrpc_core as jsonrpc;
extern crate jsonrpc_server_utils;

pub extern crate hyper;

pub mod request_response;
mod handler;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::net::SocketAddr;
use std::thread;
use std::collections::HashSet;
use hyper::server;
use jsonrpc::MetaIoHandler;
use jsonrpc_server_utils::reactor::{Remote, UnitializedRemote};
use parking_lot::Mutex;

pub use jsonrpc_server_utils::hosts::{Host, DomainsValidation};
pub use jsonrpc_server_utils::cors::AccessControlAllowOrigin;
pub use handler::{PanicHandler, ServerHandler};

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

/// Action undertaken by a middleware.
pub enum RequestMiddlewareAction {
	/// Proceed with standard RPC handling
	Proceed,
	/// Intercept the request and respond differently.
	Respond {
		/// Should standard hosts validation be performed?
		should_validate_hosts: bool,
		/// hyper handler used to process the request
		handler: Box<hyper::server::Handler<hyper::net::HttpStream> + Send>,
	}
}

impl<T: hyper::server::Handler<hyper::net::HttpStream> + Send + 'static> From<Option<T>> for RequestMiddlewareAction {
	fn from(o: Option<T>) -> Self {
		match o {
			None => RequestMiddlewareAction::Proceed,
			Some(handler) => RequestMiddlewareAction::Respond {
				should_validate_hosts: true,
				handler: Box::new(handler),
			},
		}
	}
}

/// Allows to intercept request and handle it differently.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Takes a request and decides how to proceed with it.
	fn on_request(&self, request: &server::Request<hyper::net::HttpStream>) -> RequestMiddlewareAction;
}

impl<F> RequestMiddleware for F where
	F: Fn(&server::Request<hyper::net::HttpStream>) -> RequestMiddlewareAction + Sync + Send + 'static,
{
	fn on_request(&self, request: &server::Request<hyper::net::HttpStream>) -> RequestMiddlewareAction {
		(*self)(request)
	}
}

#[derive(Default)]
struct NoopRequestMiddleware;
impl RequestMiddleware for NoopRequestMiddleware {
	fn on_request(&self, _request: &server::Request<hyper::net::HttpStream>) -> RequestMiddlewareAction {
		RequestMiddlewareAction::Proceed
	}
}

/// Extracts metadata from the HTTP request.
pub trait HttpMetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &server::Request<hyper::net::HttpStream>) -> M {
		Default::default()
	}
}

impl<M, F> HttpMetaExtractor<M> for F where
	M: jsonrpc::Metadata,
	F: Fn(&server::Request<hyper::net::HttpStream>) -> M + Sync + Send + 'static,
{
	fn read_metadata(&self, req: &server::Request<hyper::net::HttpStream>) -> M {
		(*self)(req)
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata> HttpMetaExtractor<M> for NoopExtractor {}

/// RPC Handler bundled with metadata extractor.
pub struct Rpc<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	/// RPC Handler
	pub handler: Arc<MetaIoHandler<M, S>>,
	/// Remote
	pub remote: jsonrpc_server_utils::tokio_core::reactor::Remote,
	/// Metadata extractor
	pub extractor: Arc<HttpMetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Clone for Rpc<M, S> {
	fn clone(&self) -> Self {
		Rpc {
			handler: self.handler.clone(),
			remote: self.remote.clone(),
			extractor: self.extractor.clone(),
		}
	}
}


/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	handler: MetaIoHandler<M, S>,
	remote: UnitializedRemote,
	meta_extractor: Arc<HttpMetaExtractor<M>>,
	request_middleware: Arc<RequestMiddleware>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<Host>>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// If you want to re-use the same handler in couple places
	/// see `with_remote` function.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self where
		T: Into<MetaIoHandler<M, S>>
	{
		ServerBuilder {
			handler: handler.into(),
			remote: UnitializedRemote::Unspawned,
			meta_extractor: Arc::new(NoopExtractor::default()),
			request_middleware: Arc::new(NoopRequestMiddleware::default()),
			cors_domains: None,
			allowed_hosts: None,
			panic_handler: None,
		}
	}

	/// Utilize existing event loop remote to poll RPC results.
	pub fn event_loop_remote(mut self, remote: jsonrpc_server_utils::tokio_core::reactor::Remote) -> Self {
		self.remote = UnitializedRemote::Shared(remote);
		self
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

	/// Configures request middleware
	pub fn request_middleware<T: RequestMiddleware>(mut self, middleware: T) -> Self {
		self.request_middleware = Arc::new(middleware);
		self
	}

	/// Configures metadata extractor
	pub fn meta_extractor<T: HttpMetaExtractor<M>>(mut self, extractor: T) -> Self {
		self.meta_extractor = Arc::new(extractor);
		self
	}

	/// Allow connections only with `Host` header set to binding address.
	pub fn allow_only_bind_host(mut self) -> Self {
		self.allowed_hosts = Some(Vec::new());
		self
	}

	/// Specify a list of valid `Host` headers. Binding address is allowed automatically.
	pub fn allowed_hosts(mut self, allowed_hosts: DomainsValidation<Host>) -> Self {
		self.allowed_hosts = allowed_hosts.into();
		self
	}

	/// Start this JSON-RPC HTTP server trying to bind to specified `SocketAddr`.
	pub fn start_http(self, addr: &SocketAddr) -> ServerResult {
		let cors_domains = self.cors_domains;
		let request_middleware = self.request_middleware;
		let panic_for_server = Arc::new(Mutex::new(self.panic_handler));
		let hosts = Arc::new(Mutex::new(self.allowed_hosts));
		let hosts_setter = hosts.clone();

		let eloop = self.remote.initialize()?;
		let jsonrpc_handler = Rpc {
			handler: Arc::new(self.handler),
			remote: eloop.remote(),
			extractor: self.meta_extractor,
		};

		let (l, srv) = hyper::Server::http(addr)?.handle(move |control| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			let hosts = hosts.lock().clone();
			ServerHandler::new(
				jsonrpc_handler.clone(),
				cors_domains.clone(),
				hosts,
				request_middleware.clone(),
				handler,
				control,
			)
		})?;

		// Add current host to allowed headers.
		// NOTE: we need to use `l.addrs()` instead of `addr`
		// it might be different!
		{
			let mut hosts = hosts_setter.lock();
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
			remote: Some(eloop),
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	server: Option<server::Listening>,
	handle: Option<thread::JoinHandle<()>>,
	remote: Option<Remote>,
}

impl Server {
	/// Returns addresses of this server
	pub fn addrs(&self) -> &[SocketAddr] {
		self.server.as_ref().unwrap().addrs()
	}

	/// Closes the server.
	pub fn close(mut self) {
		self.remote.take().unwrap().close();
		self.server.take().unwrap().close();
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.remote.take().map(|remote| remote.close());
		self.server.take().map(|server| server.close());
	}
}
