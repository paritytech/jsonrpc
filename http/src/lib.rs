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
//! 	let _server = ServerBuilder::new(io)
//!		.start_http(&"127.0.0.1:3030".parse().unwrap())
//!		.expect("Unable to start RPC server");
//!
//!	_server.wait();
//! }
//! ```

#![warn(missing_docs)]

extern crate unicase;
extern crate jsonrpc_server_utils as server_utils;
extern crate net2;

pub extern crate jsonrpc_core;
pub extern crate hyper;

#[macro_use]
extern crate log;

mod handler;
mod response;
mod utils;
#[cfg(test)]
mod tests;

use std::io;
use std::sync::{mpsc, Arc};
use std::net::SocketAddr;

use hyper::server;
use jsonrpc_core as jsonrpc;
use jsonrpc::MetaIoHandler;
use jsonrpc::futures::{self, Future, Stream};
use jsonrpc::futures::sync::oneshot;
use server_utils::reactor::{Remote, UninitializedRemote};

pub use server_utils::hosts::{Host, DomainsValidation};
pub use server_utils::cors::{AccessControlAllowOrigin, Origin};
pub use server_utils::tokio_core;
pub use handler::ServerHandler;
pub use utils::{is_host_allowed, cors_header, CorsHeader};
pub use response::Response;

/// Action undertaken by a middleware.
pub enum RequestMiddlewareAction {
	/// Proceed with standard RPC handling
	Proceed {
		/// Should the request be processed even if invalid CORS headers are detected?
		/// This allows for side effects to take place.
		should_continue_on_invalid_cors: bool,
		/// The request object returned
		request: server::Request,
	},
	/// Intercept the request and respond differently.
	Respond {
		/// Should standard hosts validation be performed?
		should_validate_hosts: bool,
		/// a future for server response
		response: Box<Future<Item=server::Response, Error=hyper::Error> + Send>,
	}
}

impl From<Response> for RequestMiddlewareAction {
	fn from(o: Response) -> Self {
		RequestMiddlewareAction::Respond {
			should_validate_hosts: true,
			response: Box::new(futures::future::ok(o.into())),
		}
	}
}

impl From<server::Response> for RequestMiddlewareAction {
	fn from(response: server::Response) -> Self {
		RequestMiddlewareAction::Respond {
			should_validate_hosts: true,
			response: Box::new(futures::future::ok(response)),
		}
	}
}

impl From<server::Request> for RequestMiddlewareAction {
	fn from(request: server::Request) -> Self {
		RequestMiddlewareAction::Proceed {
			should_continue_on_invalid_cors: false,
			request,
		}
	}
}

/// Allows to intercept request and handle it differently.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Takes a request and decides how to proceed with it.
	fn on_request(&self, request: server::Request) -> RequestMiddlewareAction;
}

impl<F> RequestMiddleware for F where
	F: Fn(server::Request) -> RequestMiddlewareAction + Sync + Send + 'static,
{
	fn on_request(&self, request: server::Request) -> RequestMiddlewareAction {
		(*self)(request)
	}
}

#[derive(Default)]
struct NoopRequestMiddleware;
impl RequestMiddleware for NoopRequestMiddleware {
	fn on_request(&self, request: server::Request) -> RequestMiddlewareAction {
		RequestMiddlewareAction::Proceed {
			should_continue_on_invalid_cors: false,
			request,
		}
	}
}

/// Extracts metadata from the HTTP request.
pub trait MetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &server::Request) -> M;
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
impl<M: jsonrpc::Metadata + Default> MetaExtractor<M> for NoopExtractor {
	fn read_metadata(&self, _: &server::Request) -> M {
		M::default()
	}
}
//
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

type AllowedHosts = Option<Vec<Host>>;
type CorsDomains = Option<Vec<AccessControlAllowOrigin>>;

/// REST -> RPC converter state.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RestApi {
	/// The REST -> RPC converter is enabled
	/// and requires `Content-Type: application/json` header
	/// (even though the body should be empty).
	/// This protects from submitting an RPC call
	/// from unwanted origins.
	Secure,
	/// The REST -> RPC converter is enabled
	/// and does not require any `Content-Type` headers.
	/// NOTE: This allows sending RPCs via HTTP forms
	/// from any website.
	Unsecure,
	/// The REST -> RPC converter is disabled.
	Disabled,
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	remote: UninitializedRemote,
	meta_extractor: Arc<MetaExtractor<M>>,
	request_middleware: Arc<RequestMiddleware>,
	cors_domains: CorsDomains,
	allowed_hosts: AllowedHosts,
	rest_api: RestApi,
	keep_alive: bool,
	threads: usize,
	max_request_body_size: usize,
}

const SENDER_PROOF: &'static str = "Server initialization awaits local address.";

impl<M: jsonrpc::Metadata + Default, S: jsonrpc::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self where
		T: Into<MetaIoHandler<M, S>>
	{
		Self::with_meta_extractor(handler, NoopExtractor)
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn with_meta_extractor<T, E>(handler: T, extractor: E) -> Self where
		T: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			remote: UninitializedRemote::Unspawned,
			meta_extractor: Arc::new(extractor),
			request_middleware: Arc::new(NoopRequestMiddleware::default()),
			cors_domains: None,
			allowed_hosts: None,
			rest_api: RestApi::Disabled,
			keep_alive: true,
			threads: 1,
			max_request_body_size: 5 * 1024 * 1024,
		}
	}

	/// Utilize existing event loop remote to poll RPC results.
	/// Applies only to 1 of the threads. Other threads will spawn their own Event Loops.
	pub fn event_loop_remote(mut self, remote: tokio_core::reactor::Remote) -> Self {
		self.remote = UninitializedRemote::Shared(remote);
		self
	}

	/// Enable the REST -> RPC converter. Allows you to invoke RPCs
	/// by sending `POST /<method>/<param1>/<param2>` requests
	/// (with no body). Disabled by default.
	pub fn rest_api(mut self, rest_api: RestApi) -> Self {
		self.rest_api = rest_api;
		self
	}

	/// Sets Enables or disables HTTP keep-alive.
	/// Default is true.
	pub fn keep_alive(mut self, val: bool) -> Self {
		self.keep_alive  = val;
		self
	}

	/// Sets number of threads of the server to run.
	/// Panics when set to `0`.
	#[cfg(not(unix))]
	pub fn threads(mut self, _threads: usize) -> Self {
		warn!("Multi-threaded server is not available on Windows. Falling back to single thread.");
		self
	}

	/// Sets number of threads of the server to run.
	/// Panics when set to `0`.
	#[cfg(unix)]
	pub fn threads(mut self, threads: usize) -> Self {
		self.threads = threads;
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

	/// Sets the maximum size of a request body in bytes (default is 5 MiB).
	pub fn max_request_body_size(mut self, val: usize) -> Self {
		self.max_request_body_size = val;
		self
	}

	/// Start this JSON-RPC HTTP server trying to bind to specified `SocketAddr`.
	pub fn start_http(self, addr: &SocketAddr) -> io::Result<Server> {
		let cors_domains = self.cors_domains;
		let request_middleware = self.request_middleware;
		let allowed_hosts = self.allowed_hosts;
		let jsonrpc_handler = Rpc {
			handler: self.handler,
			extractor: self.meta_extractor,
		};
		let rest_api = self.rest_api;
		let keep_alive = self.keep_alive;
		let reuse_port = self.threads > 1;

		let (local_addr_tx, local_addr_rx) = mpsc::channel();
		let (close, shutdown_signal) = oneshot::channel();
		let eloop = self.remote.init_with_name("http.worker0")?;
		let req_max_size = self.max_request_body_size;
		serve(
			(shutdown_signal, local_addr_tx),
			eloop.remote(),
			addr.to_owned(),
			cors_domains.clone(),
			request_middleware.clone(),
			allowed_hosts.clone(),
			jsonrpc_handler.clone(),
			rest_api,
			keep_alive,
			reuse_port,
			req_max_size,
		);
		let handles = (0..self.threads - 1).map(|i| {
			let (local_addr_tx, local_addr_rx) = mpsc::channel();
			let (close, shutdown_signal) = oneshot::channel();
			let eloop = UninitializedRemote::Unspawned.init_with_name(format!("http.worker{}", i + 1))?;
			serve(
				(shutdown_signal, local_addr_tx),
				eloop.remote(),
				addr.to_owned(),
				cors_domains.clone(),
				request_middleware.clone(),
				allowed_hosts.clone(),
				jsonrpc_handler.clone(),
				rest_api,
				keep_alive,
				reuse_port,
				req_max_size,
			);
			Ok((eloop, close, local_addr_rx))
		}).collect::<io::Result<Vec<_>>>()?;

		// Wait for server initialization
		let local_addr = recv_address(local_addr_rx);
		// Wait for other threads as well.
		let mut handles = handles.into_iter().map(|(eloop, close, local_addr_rx)| {
			let _ = recv_address(local_addr_rx)?;
			Ok((eloop, close))
		}).collect::<io::Result<(Vec<_>)>>()?;
		handles.push((eloop, close));
		let (remotes, close) = handles.into_iter().unzip();

		Ok(Server {
			address: local_addr?,
			remote: Some(remotes),
			close: Some(close),
		})
	}
}

fn recv_address(local_addr_rx: mpsc::Receiver<io::Result<SocketAddr>>) -> io::Result<SocketAddr> {
	local_addr_rx.recv().map_err(|_| {
		io::Error::new(io::ErrorKind::Interrupted, "")
	})?
}

fn serve<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>>(
	signals: (oneshot::Receiver<()>, mpsc::Sender<io::Result<SocketAddr>>),
	remote: tokio_core::reactor::Remote,
	addr: SocketAddr,
	cors_domains: CorsDomains,
	request_middleware: Arc<RequestMiddleware>,
	allowed_hosts: AllowedHosts,
	jsonrpc_handler: Rpc<M, S>,
	rest_api: RestApi,
	keep_alive: bool,
	reuse_port: bool,
	max_request_body_size: usize,
) {
	let (shutdown_signal, local_addr_tx) = signals;
	remote.spawn(move |handle| {
		let handle1 = handle.clone();
		let bind = move || {
			let listener = match addr {
				SocketAddr::V4(_) => net2::TcpBuilder::new_v4()?,
				SocketAddr::V6(_) => net2::TcpBuilder::new_v6()?,
			};
			configure_port(reuse_port, &listener)?;
			listener.reuse_address(true)?;
			listener.bind(&addr)?;
			let listener = listener.listen(1024)?;
			let listener = tokio_core::net::TcpListener::from_listener(listener, &addr, &handle1)?;
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

			let http = {
				let mut http = server::Http::new();
				http.keep_alive(keep_alive);
				http.sleep_on_errors(true);
				http
			};
			listener.incoming()
				.for_each(move |(socket, addr)| {
					http.bind_connection(&handle, socket, addr, ServerHandler::new(
						jsonrpc_handler.clone(),
						cors_domains.clone(),
						allowed_hosts.clone(),
						request_middleware.clone(),
						rest_api,
						max_request_body_size,
					));
					Ok(())
				})
				.map_err(|e| {
					warn!("Incoming streams error, closing sever: {:?}", e);
				})
				.select(shutdown_signal.map_err(|e| {
					debug!("Shutdown signaller dropped, closing server: {:?}", e);
				}))
				.map(|_| ())
				.map_err(|_| ())
		})
	});
}

#[cfg(unix)]
fn configure_port(reuse: bool, tcp: &net2::TcpBuilder) -> io::Result<()> {
	use net2::unix::*;

	if reuse {
		try!(tcp.reuse_port(true));
	}

	Ok(())
}

#[cfg(not(unix))]
fn configure_port(_reuse: bool, _tcp: &net2::TcpBuilder) -> io::Result<()> {
    Ok(())
}

/// jsonrpc http server instance
pub struct Server {
	address: SocketAddr,
	remote: Option<Vec<Remote>>,
	close: Option<Vec<oneshot::Sender<()>>>,
}

const PROOF: &'static str = "Server is always Some until self is consumed.";
impl Server {
	/// Returns address of this server
	pub fn address(&self) -> &SocketAddr {
		&self.address
	}

	/// Closes the server.
	pub fn close(mut self) {
		for close in self.close.take().expect(PROOF) {
			let _ = close.send(());
		}

		for remote in self.remote.take().expect(PROOF) {
			remote.close();
		}
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) {
		for remote in self.remote.take().expect(PROOF) {
			remote.wait();
		}
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.remote.take().map(|remotes| {
			for remote in remotes { remote.close(); }
		});
	}
}

