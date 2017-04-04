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
extern crate jsonrpc_core as jsonrpc;
extern crate jsonrpc_server_utils as server_utils;

pub extern crate hyper;

mod response;
mod handler;
mod utils;
#[cfg(test)]
mod tests;

use std::{fmt, io};
use std::sync::{mpsc, Arc};
use std::net::SocketAddr;

use hyper::server;
use jsonrpc::MetaIoHandler;
use jsonrpc::futures::{self, Future, IntoFuture, BoxFuture, Stream};
use server_utils::reactor::{Remote, UninitializedRemote};

pub use server_utils::hosts::{Host, DomainsValidation};
pub use server_utils::cors::{AccessControlAllowOrigin, Origin};
pub use server_utils::tokio_core;
pub use handler::ServerHandler;
pub use utils::{is_host_allowed, cors_header, CorsHeader};
pub use response::Response;

/// Result of starting the Server.
pub type ServerResult = Result<Server, Error>;

/// RPC Server startup error.
#[derive(Debug)]
pub enum Error {
	/// IO Error
	Io(std::io::Error),
	/// Other Error (hyper)
	Other(hyper::error::Error),
}

impl From<std::io::Error> for Error {
	fn from(err: std::io::Error) -> Self {
		Error::Io(err)
	}
}

impl From<hyper::error::Error> for Error {
	fn from(err: hyper::error::Error) -> Self {
		match err {
			hyper::error::Error::Io(e) => Error::Io(e),
			e => Error::Other(e)
		}
	}
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref e) => e.fmt(f),
            Error::Other(ref e) => e.fmt(f),
        }
    }
}

impl ::std::error::Error for Error {
    fn description(&self) -> &str {
        "Starting the JSON-RPC HTTP server failed"
    }

    fn cause(&self) -> Option<&::std::error::Error> {
        Some(match *self {
            Error::Io(ref e) => e,
            Error::Other(ref e) => e,
        })
    }
}

/// Action undertaken by a middleware.
pub enum RequestMiddlewareAction {
	/// Proceed with standard RPC handling
	Proceed {
		/// Should the request be processed even if invalid CORS headers are detected?
		/// This allows for side effects to take place.
		should_continue_on_invalid_cors: bool,
	},
	/// Intercept the request and respond differently.
	Respond {
		/// Should standard hosts validation be performed?
		should_validate_hosts: bool,
		/// hyper handler used to process the request
		handler: BoxFuture<server::Response, hyper::Error>,
	}
}

impl From<Option<Response>> for RequestMiddlewareAction {
	fn from(o: Option<Response>) -> Self {
		o.map(Into::<server::Response>::into).map(futures::future::ok).into()
	}
}
impl<T> From<Option<T>> for RequestMiddlewareAction where
	T: IntoFuture<Item=server::Response, Error=hyper::Error>,
	T::Future: Send + 'static,
{
	fn from(o: Option<T>) -> Self {
		match o {
			None => RequestMiddlewareAction::Proceed {
				should_continue_on_invalid_cors: false,
			},
			Some(handler) => RequestMiddlewareAction::Respond {
				should_validate_hosts: true,
				handler: handler.into_future().boxed(),
			},
		}
	}
}

/// Allows to intercept request and handle it differently.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Takes a request and decides how to proceed with it.
	fn on_request(&self, request: &server::Request) -> RequestMiddlewareAction;
}

impl<F> RequestMiddleware for F where
	F: Fn(&server::Request) -> RequestMiddlewareAction + Sync + Send + 'static,
{
	fn on_request(&self, request: &server::Request) -> RequestMiddlewareAction {
		(*self)(request)
	}
}

#[derive(Default)]
struct NoopRequestMiddleware;
impl RequestMiddleware for NoopRequestMiddleware {
	fn on_request(&self, _request: &server::Request) -> RequestMiddlewareAction {
		RequestMiddlewareAction::Proceed {
			should_continue_on_invalid_cors: false,
		}
	}
}

/// Extracts metadata from the HTTP request.
pub trait MetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &server::Request) -> M {
		Default::default()
	}
}

impl<M, F> MetaExtractor<M> for F where
	M: jsonrpc::Metadata,
	F: Fn(&server::Request) -> M + Sync + Send + 'static,
{
	fn read_metadata(&self, req: &server::Request) -> M {
		(*self)(req)
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata> MetaExtractor<M> for NoopExtractor {}

/// RPC Handler bundled with metadata extractor.
pub struct Rpc<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	/// RPC Handler
	pub handler: Arc<MetaIoHandler<M, S>>,
	/// Metadata extractor
	pub extractor: Arc<MetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Clone for Rpc<M, S> {
	fn clone(&self) -> Self {
		Rpc {
			handler: self.handler.clone(),
			extractor: self.extractor.clone(),
		}
	}
}


/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	remote: UninitializedRemote,
	meta_extractor: Arc<MetaExtractor<M>>,
	request_middleware: Arc<RequestMiddleware>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<Host>>,
}

const SENDER_PROOF: &'static str = "Server initialization awaits local address.";

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
			handler: Arc::new(handler.into()),
			remote: UninitializedRemote::Unspawned,
			meta_extractor: Arc::new(NoopExtractor::default()),
			request_middleware: Arc::new(NoopRequestMiddleware::default()),
			cors_domains: None,
			allowed_hosts: None,
		}
	}

	/// Utilize existing event loop remote to poll RPC results.
	pub fn event_loop_remote(mut self, remote: tokio_core::reactor::Remote) -> Self {
		self.remote = UninitializedRemote::Shared(remote);
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
	pub fn meta_extractor<T: MetaExtractor<M>>(mut self, extractor: T) -> Self {
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
		let allowed_hosts = self.allowed_hosts;

		let eloop = self.remote.initialize()?;
		let jsonrpc_handler = Rpc {
			handler: self.handler,
			extractor: self.meta_extractor,
		};

		let (local_addr_tx, local_addr_rx) = mpsc::channel();
		let (close, shutdown_signal) = futures::sync::oneshot::channel();
		let addr = addr.to_owned();
		// TODO [ToDr] Consider spawning many threads like in minihttp?
		eloop.remote().spawn(move |handle| {
			let handle1 = handle.clone();
			let bind = move || {
				let listener = tokio_core::net::TcpListener::bind(&addr, &handle1)?;
				// Add current host to allowed headers.
				// NOTE: we need to use `l.local_addr()` instead of `addr`
				// it might be different!
				let local_addr = listener.local_addr()?;

				Ok((listener, local_addr))
			};

			let bind_result = match bind() {
				Ok((listener, local_addr)) => {
					// Send local address
					local_addr_tx.send(Ok(local_addr)).expect(SENDER_PROOF);

					futures::future::ok((listener, local_addr))
				},
				Err(err) => {
					// Send error
					local_addr_tx.send(Err(err)).expect(SENDER_PROOF);

					futures::future::err(())
				}
			};

			let handle = handle.clone();
			bind_result.and_then(move |(listener, local_addr)| {
				let allowed_hosts = server_utils::hosts::update(allowed_hosts, &local_addr);

				let http = server::Http::new();
				listener.incoming()
					.for_each(move |(socket, addr)| {
						http.bind_connection(&handle, socket, addr, ServerHandler::new(
							jsonrpc_handler.clone(),
							cors_domains.clone(),
							allowed_hosts.clone(),
							request_middleware.clone(),
						));
						Ok(())
					})
					.map_err(|e| {
						warn!("Incoming streams error, closing sever: {:?}", e);
					})
					.select(shutdown_signal.map_err(|e| {
						warn!("Shutdown signaller dropped, closing server: {:?}", e);
					}))
					.map(|_| ())
					.map_err(|_| ())
			})
		});

		// Wait for server initialization
		let local_addr: io::Result<SocketAddr> = local_addr_rx.recv().map_err(|_| {
			Error::Io(io::Error::new(io::ErrorKind::Interrupted, ""))
		})?;

		Ok(Server {
			address: local_addr?,
			remote: Some(eloop),
			close: Some(close),
		})
	}
}

/// jsonrpc http server instance
pub struct Server {
	address: SocketAddr,
	remote: Option<Remote>,
	close: Option<futures::sync::oneshot::Sender<()>>,
}

const PROOF: &'static str = "Server is always Some until self is consumed.";
impl Server {
	/// Returns address of this server
	pub fn address(&self) -> &SocketAddr {
		&self.address
	}

	/// Closes the server.
	pub fn close(mut self) {
		let _ = self.close.take().expect(PROOF).send(());
		self.remote.take().expect(PROOF).close();
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) {
		self.remote.take().expect(PROOF).wait();
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.remote.take().map(|remote| remote.close());
	}
}

