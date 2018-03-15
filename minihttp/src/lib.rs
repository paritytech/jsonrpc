//! jsonrpc http server.
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_minihttp_server;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_minihttp_server::*;
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

extern crate bytes;
extern crate jsonrpc_server_utils;
extern crate parking_lot;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_minihttp;

pub extern crate jsonrpc_core;

#[macro_use]
extern crate log;

mod req;
mod res;
#[cfg(test)]
mod tests;

use std::io;
use std::sync::{Arc, mpsc};
use std::net::SocketAddr;
use std::thread;
use parking_lot::RwLock;
use jsonrpc_core as jsonrpc;
use jsonrpc::futures::{self, future, Future};
use jsonrpc::{FutureResult, MetaIoHandler, Response};
use jsonrpc_server_utils::hosts;

pub use jsonrpc_server_utils::cors;
pub use jsonrpc_server_utils::hosts::{Host, DomainsValidation};
pub use req::Req;

/// Extracts metadata from the HTTP request.
pub trait MetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
	/// Read the metadata from the request
	fn read_metadata(&self, _: &req::Req) -> M;
}

impl<M, F> MetaExtractor<M> for F where
	M: jsonrpc::Metadata,
	F: Fn(&req::Req) -> M + Sync + Send  + 'static,
{
	fn read_metadata(&self, req: &req::Req) -> M {
		(*self)(req)
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata + Default> MetaExtractor<M> for NoopExtractor {
	fn read_metadata(&self, _: &req::Req) -> M {
		M::default()
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	jsonrpc_handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<Host>>,
	threads: usize,
}

const SENDER_PROOF: &'static str = "Server initialization awaits local address.";

impl<M: jsonrpc::Metadata + Default, S: jsonrpc::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder` for given `IoHandler`.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn new<T>(handler: T) -> Self where T: Into<MetaIoHandler<M, S>> {
		Self::with_meta_extractor(handler, NoopExtractor)
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder` for given `IoHandler` and meta extractor.
	///
	/// By default:
	/// 1. Server is not sending any CORS headers.
	/// 2. Server is validating `Host` header.
	pub fn with_meta_extractor<T, E>(handler: T, extractor: E) -> Self where
		T: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M>,
	{
		ServerBuilder {
			jsonrpc_handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(extractor),
			cors_domains: None,
			allowed_hosts: None,
			threads: 1,
		}
	}

	/// Sets number of threads of the server to run. (not available for windows)
	/// Panics when set to `0`.
	pub fn threads(mut self, threads: usize) -> Self {
		assert!(threads > 0);
		self.threads = threads;
		self
	}

	/// Configures a list of allowed CORS origins.
	pub fn cors(mut self, cors_domains: DomainsValidation<cors::AccessControlAllowOrigin>) -> Self {
		self.cors_domains = cors_domains.into();
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
	pub fn start_http(self, addr: &SocketAddr) -> io::Result<Server> {
		let cors_domains = self.cors_domains;
		let allowed_hosts = self.allowed_hosts;
		let handler = self.jsonrpc_handler;
		let meta_extractor = self.meta_extractor;
		let threads = self.threads;

		let (local_addr_tx, local_addr_rx) = mpsc::channel();
		let (close, shutdown_signal) = futures::sync::oneshot::channel();
		let addr = addr.to_owned();
		let handle = thread::spawn(move || {
			let run = move || {
				let hosts = Arc::new(RwLock::new(allowed_hosts.clone()));
				let hosts2 = hosts.clone();
				let mut hosts_setter = hosts2.write();

				let mut server = tokio_proto::TcpServer::new(tokio_minihttp::Http, addr);
				server.threads(threads);
				let server = server.bind(move || Ok(RpcService {
					handler: handler.clone(),
					meta_extractor: meta_extractor.clone(),
					hosts: hosts.read().clone(),
					cors_domains: cors_domains.clone(),
				}))?;

				let local_addr = server.local_addr()?;
				// Add current host to allowed headers.
				// NOTE: we need to use `local_address` instead of `addr`
				// it might be different!
				*hosts_setter = hosts::update(allowed_hosts, &local_addr);

				Ok((server, local_addr))
			};

			match run() {
				Ok((server, local_addr)) => {
					// Send local address
					local_addr_tx.send(Ok(local_addr)).expect(SENDER_PROOF);

					// Start the server and wait for shutdown signal
					server.run_until(shutdown_signal.map_err(|_| {
						warn!("Shutdown signaller dropped, closing server.");
					})).expect("Expected clean shutdown.")
				},
				Err(err) => {
					// Send error
					local_addr_tx.send(Err(err)).expect(SENDER_PROOF);
				}
			}
		});

		// Wait for server initialization
		let local_addr: io::Result<SocketAddr> = local_addr_rx.recv().map_err(|_| {
			io::Error::new(io::ErrorKind::Interrupted, "")
		})?;

		Ok(Server {
			address: local_addr?,
			handle: Some(handle),
			close: Some(close),
		})
	}
}

/// Tokio-proto JSON-RPC HTTP Service
pub struct RpcService<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	hosts: Option<Vec<Host>>,
	cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
}

fn is_json(content_type: Option<&str>) -> bool {
	match content_type {
		None => false,
		Some(ref content_type) => {
			let json = "application/json";
			content_type.eq_ignore_ascii_case(json)
		}
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> tokio_service::Service for RpcService<M, S> {
	type Request = tokio_minihttp::Request;
	type Response = tokio_minihttp::Response;
	type Error = io::Error;
	type Future = future::Either<
		future::FutureResult<tokio_minihttp::Response, io::Error>,
		RpcResponse<S::Future>,
	>;

	fn call(&self, request: Self::Request) -> Self::Future {
		use self::future::Either;

		let request = req::Req::new(request);
		// Validate HTTP Method
		let is_options = request.method() == req::Method::Options;
		if !is_options && request.method() != req::Method::Post {
			return Either::A(future::ok(
				res::method_not_allowed()
			));
		}

		// Validate allowed hosts
		let host = request.header("Host");
		if !hosts::is_host_valid(host.clone(), &self.hosts) {
			return Either::A(future::ok(
				res::invalid_host()
			));
		}

		// Extract CORS headers
		let origin = request.header("Origin");
		let cors = cors::get_cors_header(origin, host, &self.cors_domains);

		// Validate cors header
		if let cors::CorsHeader::Invalid = cors {
			return Either::A(future::ok(
				res::invalid_cors()
			));
		}

		// Don't process data if it's OPTIONS
		if is_options {
			return Either::A(future::ok(
				res::options(cors.into())
			));
		}

		// Validate content type
		let content_type = request.header("Content-type");
		if !is_json(content_type) {
			return Either::A(future::ok(
				res::invalid_content_type()
			));
		}

		// Extract metadata
		let metadata = self.meta_extractor.read_metadata(&request);

		// Read & handle request
		let data = request.body();
		let future = self.handler.handle_request(data, metadata);
		Either::B(RpcResponse {
			future: future,
			cors: cors.into(),
		})
	}
}

/// RPC response wrapper
pub struct RpcResponse<F: Future<Item = Option<Response>, Error = ()>> {
	future: FutureResult<F>,
	cors: Option<cors::AccessControlAllowOrigin>,
}

impl<F: Future<Item = Option<Response>, Error = ()>> Future for RpcResponse<F> {
	type Item = tokio_minihttp::Response;
	type Error = io::Error;

	fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
		use self::futures::Async::*;

		match self.future.poll() {
			Err(_) => Ok(Ready(res::internal_error())),
			Ok(NotReady) => Ok(NotReady),
			Ok(Ready(result)) => {
				let result = format!("{}\n", result.unwrap_or_default());
				Ok(Ready(res::new(&result, self.cors.take())))
			},
		}
	}
}

/// jsonrpc http server instance
pub struct Server {
	address: SocketAddr,
	handle: Option<thread::JoinHandle<()>>,
	close: Option<futures::sync::oneshot::Sender<()>>,
}

impl Server {
	/// Returns addresses of this server
	pub fn address(&self) -> &SocketAddr {
		&self.address
	}

	/// Closes the server.
	pub fn close(mut self) {
		let _ = self.close.take().expect("Close is always set before self is consumed.").send(());
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().expect("Handle is always set before set is consumed.").join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.close.take().map(|close| close.send(()));
	}
}
