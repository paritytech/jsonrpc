//! jsonrpc http server.
//!
//! ```no_run
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//!
//! fn main() {
//!     let mut io = IoHandler::new();
//!     io.add_sync_method("say_hello", |_: Params| {
//!         Ok(Value::String("hello".to_string()))
//!     });
//!
//!     let _server = ServerBuilder::new(io)
//!     .start_http(&"127.0.0.1:3030".parse().unwrap())
//!     .expect("Unable to start RPC server");
//!
//! _server.wait();
//! }
//! ```

#![deny(missing_docs)]

use jsonrpc_server_utils as server_utils;

pub use hyper;
pub use jsonrpc_core;

#[macro_use]
extern crate log;

mod handler;
mod response;
#[cfg(test)]
mod tests;
mod utils;

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{mpsc, Arc, Weak};
use std::thread;

use parking_lot::Mutex;

use crate::jsonrpc::MetaIoHandler;
use crate::server_utils::reactor::{Executor, UninitializedExecutor};
use futures::{channel::oneshot, future};
use hyper::Body;
use jsonrpc_core as jsonrpc;

pub use crate::handler::ServerHandler;
pub use crate::response::Response;
pub use crate::server_utils::cors::{self, AccessControlAllowOrigin, AllowCors, Origin};
pub use crate::server_utils::hosts::{DomainsValidation, Host};
pub use crate::server_utils::reactor::TaskExecutor;
pub use crate::server_utils::{tokio, SuspendableStream};
pub use crate::utils::{cors_allow_headers, cors_allow_origin, is_host_allowed};

/// Action undertaken by a middleware.
pub enum RequestMiddlewareAction {
	/// Proceed with standard RPC handling
	Proceed {
		/// Should the request be processed even if invalid CORS headers are detected?
		/// This allows for side effects to take place.
		should_continue_on_invalid_cors: bool,
		/// The request object returned
		request: hyper::Request<Body>,
	},
	/// Intercept the request and respond differently.
	Respond {
		/// Should standard hosts validation be performed?
		should_validate_hosts: bool,
		/// a future for server response
		response: Pin<Box<dyn Future<Output = hyper::Result<hyper::Response<Body>>> + Send>>,
	},
}

impl From<Response> for RequestMiddlewareAction {
	fn from(o: Response) -> Self {
		RequestMiddlewareAction::Respond {
			should_validate_hosts: true,
			response: Box::pin(async { Ok(o.into()) }),
		}
	}
}

impl From<hyper::Response<Body>> for RequestMiddlewareAction {
	fn from(response: hyper::Response<Body>) -> Self {
		RequestMiddlewareAction::Respond {
			should_validate_hosts: true,
			response: Box::pin(async { Ok(response) }),
		}
	}
}

impl From<hyper::Request<Body>> for RequestMiddlewareAction {
	fn from(request: hyper::Request<Body>) -> Self {
		RequestMiddlewareAction::Proceed {
			should_continue_on_invalid_cors: false,
			request,
		}
	}
}

/// Allows to intercept request and handle it differently.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Takes a request and decides how to proceed with it.
	fn on_request(&self, request: hyper::Request<hyper::Body>) -> RequestMiddlewareAction;
}

impl<F> RequestMiddleware for F
where
	F: Fn(hyper::Request<Body>) -> RequestMiddlewareAction + Sync + Send + 'static,
{
	fn on_request(&self, request: hyper::Request<hyper::Body>) -> RequestMiddlewareAction {
		(*self)(request)
	}
}

#[derive(Default)]
struct NoopRequestMiddleware;
impl RequestMiddleware for NoopRequestMiddleware {
	fn on_request(&self, request: hyper::Request<Body>) -> RequestMiddlewareAction {
		RequestMiddlewareAction::Proceed {
			should_continue_on_invalid_cors: false,
			request,
		}
	}
}

/// Extracts metadata from the HTTP request.
pub trait MetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &hyper::Request<Body>) -> M;
}

impl<M, F> MetaExtractor<M> for F
where
	M: jsonrpc::Metadata,
	F: Fn(&hyper::Request<Body>) -> M + Sync + Send + 'static,
{
	fn read_metadata(&self, req: &hyper::Request<Body>) -> M {
		(*self)(req)
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata + Default> MetaExtractor<M> for NoopExtractor {
	fn read_metadata(&self, _: &hyper::Request<Body>) -> M {
		M::default()
	}
}
//
/// RPC Handler bundled with metadata extractor.
pub struct Rpc<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::middleware::Noop> {
	/// RPC Handler
	pub handler: Arc<MetaIoHandler<M, S>>,
	/// Metadata extractor
	pub extractor: Arc<dyn MetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Clone for Rpc<M, S> {
	fn clone(&self) -> Self {
		Rpc {
			handler: self.handler.clone(),
			extractor: self.extractor.clone(),
		}
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Rpc<M, S> {
	/// Downgrade the `Rpc` to `WeakRpc`.
	///
	/// Downgrades internal `Arc`s to `Weak` references.
	pub fn downgrade(&self) -> WeakRpc<M, S> {
		WeakRpc {
			handler: Arc::downgrade(&self.handler),
			extractor: Arc::downgrade(&self.extractor),
		}
	}
}
/// A weak handle to the RPC server.
///
/// Since request handling futures are spawned directly on the executor,
/// whenever the server is closed we want to make sure that existing
/// tasks are not blocking the server and are dropped as soon as the server stops.
pub struct WeakRpc<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::middleware::Noop> {
	handler: Weak<MetaIoHandler<M, S>>,
	extractor: Weak<dyn MetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Clone for WeakRpc<M, S> {
	fn clone(&self) -> Self {
		WeakRpc {
			handler: self.handler.clone(),
			extractor: self.extractor.clone(),
		}
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> WeakRpc<M, S> {
	/// Upgrade the handle to a strong one (`Rpc`) if  possible.
	pub fn upgrade(&self) -> Option<Rpc<M, S>> {
		let handler = self.handler.upgrade()?;
		let extractor = self.extractor.upgrade()?;

		Some(Rpc { handler, extractor })
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
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	executor: UninitializedExecutor,
	meta_extractor: Arc<dyn MetaExtractor<M>>,
	request_middleware: Arc<dyn RequestMiddleware>,
	cors_domains: CorsDomains,
	cors_max_age: Option<u32>,
	allowed_headers: cors::AccessControlAllowHeaders,
	allowed_hosts: AllowedHosts,
	rest_api: RestApi,
	health_api: Option<(String, String)>,
	keep_alive: bool,
	threads: usize,
	max_request_body_size: usize,
}

impl<M: jsonrpc::Metadata + Default, S: jsonrpc::Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self
	where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_meta_extractor(handler, NoopExtractor)
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn with_meta_extractor<T, E>(handler: T, extractor: E) -> Self
	where
		T: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			executor: UninitializedExecutor::Unspawned,
			meta_extractor: Arc::new(extractor),
			request_middleware: Arc::new(NoopRequestMiddleware::default()),
			cors_domains: None,
			cors_max_age: None,
			allowed_headers: cors::AccessControlAllowHeaders::Any,
			allowed_hosts: None,
			rest_api: RestApi::Disabled,
			health_api: None,
			keep_alive: true,
			threads: 1,
			max_request_body_size: 5 * 1024 * 1024,
		}
	}

	/// Utilize existing event loop executor to poll RPC results.
	///
	/// Applies only to 1 of the threads. Other threads will spawn their own Event Loops.
	pub fn event_loop_executor(mut self, executor: TaskExecutor) -> Self {
		self.executor = UninitializedExecutor::Shared(executor);
		self
	}

	/// Enable the REST -> RPC converter.
	///
	/// Allows you to invoke RPCs by sending `POST /<method>/<param1>/<param2>` requests
	/// (with no body). Disabled by default.
	pub fn rest_api(mut self, rest_api: RestApi) -> Self {
		self.rest_api = rest_api;
		self
	}

	/// Enable health endpoint.
	///
	/// Allows you to expose one of the methods under `GET /<path>`
	/// The method will be invoked with no parameters.
	/// Error returned from the method will be converted to status `500` response.
	///
	/// Expects a tuple with `(<path>, <rpc-method-name>)`.
	pub fn health_api<A, B, T>(mut self, health_api: T) -> Self
	where
		T: Into<Option<(A, B)>>,
		A: Into<String>,
		B: Into<String>,
	{
		self.health_api = health_api.into().map(|(a, b)| (a.into(), b.into()));
		self
	}

	/// Enables or disables HTTP keep-alive.
	///
	/// Default is true.
	pub fn keep_alive(mut self, val: bool) -> Self {
		self.keep_alive = val;
		self
	}

	/// Sets number of threads of the server to run.
	///
	/// Panics when set to `0`.
	#[cfg(not(unix))]
	#[allow(unused_mut)]
	pub fn threads(mut self, _threads: usize) -> Self {
		warn!("Multi-threaded server is not available on Windows. Falling back to single thread.");
		self
	}

	/// Sets number of threads of the server to run.
	///
	/// Panics when set to `0`.
	/// The first thread will use provided `Executor` instance
	/// and all other threads will use `UninitializedExecutor` to spawn
	/// a new runtime for futures.
	/// So it's also possible to run a multi-threaded server by
	/// passing the default `tokio::runtime` executor to this builder
	/// and setting `threads` to 1.
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

	/// Configure CORS `AccessControlMaxAge` header returned.
	///
	/// Informs the client that the CORS preflight request is not necessary for `cors_max_age` seconds.
	/// Disabled by default.
	pub fn cors_max_age<T: Into<Option<u32>>>(mut self, cors_max_age: T) -> Self {
		self.cors_max_age = cors_max_age.into();
		self
	}

	/// Configure the CORS `AccessControlAllowHeaders` header which are allowed.
	pub fn cors_allow_headers(mut self, allowed_headers: cors::AccessControlAllowHeaders) -> Self {
		self.allowed_headers = allowed_headers;
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
		let cors_max_age = self.cors_max_age;
		let allowed_headers = self.allowed_headers;
		let request_middleware = self.request_middleware;
		let allowed_hosts = self.allowed_hosts;
		let jsonrpc_handler = Rpc {
			handler: self.handler,
			extractor: self.meta_extractor,
		};
		let rest_api = self.rest_api;
		let health_api = self.health_api;
		let keep_alive = self.keep_alive;
		let reuse_port = self.threads > 1;

		let (local_addr_tx, local_addr_rx) = mpsc::channel();
		let (close, shutdown_signal) = oneshot::channel();
		let (done_tx, done_rx) = oneshot::channel();
		let eloop = self.executor.init_with_name("http.worker0")?;
		let req_max_size = self.max_request_body_size;
		// The first threads `Executor` is initialised differently from the others
		serve(
			(shutdown_signal, local_addr_tx, done_tx),
			eloop.executor(),
			addr.to_owned(),
			cors_domains.clone(),
			cors_max_age,
			allowed_headers.clone(),
			request_middleware.clone(),
			allowed_hosts.clone(),
			jsonrpc_handler.clone(),
			rest_api,
			health_api.clone(),
			keep_alive,
			reuse_port,
			req_max_size,
		);
		let handles = (0..self.threads - 1)
			.map(|i| {
				let (local_addr_tx, local_addr_rx) = mpsc::channel();
				let (close, shutdown_signal) = oneshot::channel();
				let (done_tx, done_rx) = oneshot::channel();
				let eloop = UninitializedExecutor::Unspawned.init_with_name(format!("http.worker{}", i + 1))?;
				serve(
					(shutdown_signal, local_addr_tx, done_tx),
					eloop.executor(),
					addr.to_owned(),
					cors_domains.clone(),
					cors_max_age,
					allowed_headers.clone(),
					request_middleware.clone(),
					allowed_hosts.clone(),
					jsonrpc_handler.clone(),
					rest_api,
					health_api.clone(),
					keep_alive,
					reuse_port,
					req_max_size,
				);
				Ok((eloop, close, local_addr_rx, done_rx))
			})
			.collect::<io::Result<Vec<_>>>()?;

		// Wait for server initialization
		let local_addr = recv_address(local_addr_rx);
		// Wait for other threads as well.
		let mut handles: Vec<(Executor, oneshot::Sender<()>, oneshot::Receiver<()>)> = handles
			.into_iter()
			.map(|(eloop, close, local_addr_rx, done_rx)| {
				let _ = recv_address(local_addr_rx)?;
				Ok((eloop, close, done_rx))
			})
			.collect::<io::Result<Vec<_>>>()?;
		handles.push((eloop, close, done_rx));

		let (executors, done_rxs) = handles
			.into_iter()
			.fold((vec![], vec![]), |mut acc, (eloop, closer, done_rx)| {
				acc.0.push((eloop, closer));
				acc.1.push(done_rx);
				acc
			});

		Ok(Server {
			address: local_addr?,
			executors: Arc::new(Mutex::new(Some(executors))),
			done: Some(done_rxs),
		})
	}
}

fn recv_address(local_addr_rx: mpsc::Receiver<io::Result<SocketAddr>>) -> io::Result<SocketAddr> {
	local_addr_rx
		.recv()
		.map_err(|_| io::Error::new(io::ErrorKind::Interrupted, ""))?
}

fn serve<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>>(
	signals: (
		oneshot::Receiver<()>,
		mpsc::Sender<io::Result<SocketAddr>>,
		oneshot::Sender<()>,
	),
	executor: TaskExecutor,
	addr: SocketAddr,
	cors_domains: CorsDomains,
	cors_max_age: Option<u32>,
	allowed_headers: cors::AccessControlAllowHeaders,
	request_middleware: Arc<dyn RequestMiddleware>,
	allowed_hosts: AllowedHosts,
	jsonrpc_handler: Rpc<M, S>,
	rest_api: RestApi,
	health_api: Option<(String, String)>,
	keep_alive: bool,
	reuse_port: bool,
	max_request_body_size: usize,
) where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	let (shutdown_signal, local_addr_tx, done_tx) = signals;
	executor.spawn(async move {
		let bind = move || {
			let listener = match addr {
				SocketAddr::V4(_) => net2::TcpBuilder::new_v4()?,
				SocketAddr::V6(_) => net2::TcpBuilder::new_v6()?,
			};
			configure_port(reuse_port, &listener)?;
			listener.reuse_address(true)?;
			listener.bind(&addr)?;
			let listener = listener.listen(1024)?;
			let local_addr = listener.local_addr()?;

			// NOTE: Future-proof by explicitly setting the listener socket to
			// non-blocking mode of operation (future Tokio/Hyper versions
			// require for the callers to do that manually)
			listener.set_nonblocking(true)?;
			// HACK: See below.
			#[cfg(windows)]
			let raw_socket = std::os::windows::io::AsRawSocket::as_raw_socket(&listener);
			#[cfg(not(windows))]
			let raw_socket = ();

			let server_builder =
				hyper::Server::from_tcp(listener).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
			// Add current host to allowed headers.
			// NOTE: we need to use `l.local_addr()` instead of `addr`
			// it might be different!
			Ok((server_builder, local_addr, raw_socket))
		};

		let bind_result = match bind() {
			Ok((server_builder, local_addr, raw_socket)) => {
				// Send local address
				match local_addr_tx.send(Ok(local_addr)) {
					Ok(_) => Ok((server_builder, local_addr, raw_socket)),
					Err(_) => {
						warn!(
							"Thread {:?} unable to reach receiver, closing server",
							thread::current().name()
						);
						Err(())
					}
				}
			}
			Err(err) => {
				// Send error
				let _send_result = local_addr_tx.send(Err(err));

				Err(())
			}
		};

		let (server_builder, local_addr, _raw_socket) = bind_result?;

		let allowed_hosts = server_utils::hosts::update(allowed_hosts, &local_addr);

		let server_builder = server_builder
			.http1_keepalive(keep_alive)
			.tcp_nodelay(true)
			// Explicitly attempt to recover from accept errors (e.g. too many
			// files opened) instead of erroring out the entire server.
			.tcp_sleep_on_accept_errors(true);

		let service_fn = hyper::service::make_service_fn(move |_addr_stream| {
			let service = ServerHandler::new(
				jsonrpc_handler.downgrade(),
				cors_domains.clone(),
				cors_max_age,
				allowed_headers.clone(),
				allowed_hosts.clone(),
				request_middleware.clone(),
				rest_api,
				health_api.clone(),
				max_request_body_size,
				keep_alive,
			);
			async { Ok::<_, Infallible>(service) }
		});

		let server = server_builder.serve(service_fn).with_graceful_shutdown(async {
			if let Err(err) = shutdown_signal.await {
				debug!("Shutdown signaller dropped, closing server: {:?}", err);
			}
		});

		if let Err(err) = server.await {
			error!("Error running HTTP server: {:?}", err);
		}

		// FIXME: Work around TCP listener socket not being properly closed
		// in mio v0.6. This runs the std::net::TcpListener's destructor,
		// which closes the underlying OS socket.
		// Remove this once we migrate to Tokio 1.0.
		#[cfg(windows)]
		let _: std::net::TcpListener = unsafe { std::os::windows::io::FromRawSocket::from_raw_socket(_raw_socket) };

		done_tx.send(())
	});
}

#[cfg(unix)]
fn configure_port(reuse: bool, tcp: &net2::TcpBuilder) -> io::Result<()> {
	use net2::unix::*;

	if reuse {
		tcp.reuse_port(true)?;
	}

	Ok(())
}

#[cfg(not(unix))]
fn configure_port(_reuse: bool, _tcp: &net2::TcpBuilder) -> io::Result<()> {
	Ok(())
}

/// Handle used to close the server. Can be cloned and passed around to different threads and be used
/// to close a server that is `wait()`ing.

#[derive(Clone)]
pub struct CloseHandle(Arc<Mutex<Option<Vec<(Executor, oneshot::Sender<()>)>>>>);

impl CloseHandle {
	/// Shutdown a running server
	pub fn close(self) {
		if let Some(executors) = self.0.lock().take() {
			for (executor, closer) in executors {
				// First send shutdown signal so we can proceed with underlying select
				let _ = closer.send(());
				executor.close();
			}
		}
	}
}

type Executors = Arc<Mutex<Option<Vec<(Executor, oneshot::Sender<()>)>>>>;
/// jsonrpc http server instance
pub struct Server {
	address: SocketAddr,
	executors: Executors,
	done: Option<Vec<oneshot::Receiver<()>>>,
}

impl Server {
	/// Returns address of this server
	pub fn address(&self) -> &SocketAddr {
		&self.address
	}

	/// Closes the server.
	pub fn close(self) {
		self.close_handle().close()
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) {
		self.wait_internal();
	}

	/// Get a handle that allows us to close the server from a different thread and/or while the
	/// server is `wait()`ing.
	pub fn close_handle(&self) -> CloseHandle {
		CloseHandle(self.executors.clone())
	}

	fn wait_internal(&mut self) {
		if let Some(receivers) = self.done.take() {
			// NOTE: Gracefully handle the case where we may wait on a *nested*
			// local task pool (for now, wait on a dedicated, spawned thread)
			let _ = std::thread::spawn(move || futures::executor::block_on(future::try_join_all(receivers))).join();
		}
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.close_handle().close();
		self.wait_internal();
	}
}
