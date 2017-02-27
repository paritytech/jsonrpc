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

#[macro_use] extern crate log;
extern crate futures;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;
extern crate parking_lot;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_minihttp;

pub mod cors;
pub mod hosts;
mod req;
mod res;
#[cfg(test)]
mod tests;

use std::io;
use std::ascii::AsciiExt;
use std::sync::{Arc, mpsc};
use std::net::SocketAddr;
use std::thread;
use std::collections::HashSet;
use parking_lot::RwLock;
use futures::{future, Future, BoxFuture};
use jsonrpc::MetaIoHandler;

pub use req::Req;

/// Result of starting the Server.
pub type ServerResult = Result<Server, RpcServerError>;

/// RPC Server startup error.
#[derive(Debug)]
pub enum RpcServerError {
	/// IO Error
	IoError(io::Error),
}

impl From<io::Error> for RpcServerError {
	fn from(err: io::Error) -> Self {
		RpcServerError::IoError(err)
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
	fn read_metadata(&self, _: &req::Req) -> M {
		Default::default()
	}
}

impl<M, F> HttpMetaExtractor<M> for F where
	M: jsonrpc::Metadata,
	F: Fn(&req::Req) -> M + Sync + Send  + 'static,
{
	fn read_metadata(&self, req: &req::Req) -> M {
		(*self)(req)
	}
}

#[derive(Default)]
struct NoopExtractor;
impl<M: jsonrpc::Metadata> HttpMetaExtractor<M> for NoopExtractor {}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	jsonrpc_handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<HttpMetaExtractor<M>>,
	cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
	panic_handler: Option<Box<Fn() -> () + Send>>,
	threads: usize,
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
			jsonrpc_handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(NoopExtractor::default()),
			cors_domains: None,
			allowed_hosts: None,
			panic_handler: None,
			threads: 1,
		}
	}

	/// Sets number of threads of the server to run.
	/// Panics when set to `0`.
	pub fn threads(mut self, threads: usize) -> Self {
		assert!(threads > 0);
		self.threads = threads;
		self
	}

	/// Sets handler invoked in case of server panic.
	pub fn panic_handler<F>(mut self, handler: F) -> Self where F : Fn() -> () + Send + 'static {
		self.panic_handler = Some(Box::new(handler));
		self
	}

	/// Configures a list of allowed CORS origins.
	pub fn cors(mut self, cors_domains: DomainsValidation<cors::AccessControlAllowOrigin>) -> Self {
		self.cors_domains = cors_domains.into();
		self
	}

	/// Configures metadata extractor
	pub fn meta_extractor(mut self, extractor: Arc<HttpMetaExtractor<M>>) -> Self {
		self.meta_extractor = extractor;
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

	fn update_hosts(hosts: Option<Vec<String>>, address: SocketAddr) -> Option<Vec<String>> {
		hosts.map(|current_hosts| {
			// TODO [ToDr] Pre-process hosts (so that they don't contain the path)
			let mut new_hosts = current_hosts.into_iter().collect::<HashSet<_>>();
			let address = address.to_string();
			new_hosts.insert(address.clone());
			new_hosts.insert(address.replace("127.0.0.1", "localhost"));
			new_hosts.into_iter().collect()
		})
	}

	/// Start this JSON-RPC HTTP server trying to bind to specified `SocketAddr`.
	pub fn start_http(self, addr: &SocketAddr) -> ServerResult {
		let cors_domains = self.cors_domains;
		let allowed_hosts = self.allowed_hosts;
		let handler = self.jsonrpc_handler;
		let meta_extractor = self.meta_extractor;
		let panic_handler = self.panic_handler;
		let threads = self.threads;

		let (local_addr_tx, local_addr_rx) = mpsc::channel();
		let (close, shutdown_signal) = futures::sync::oneshot::channel();
		let addr = addr.to_owned();
		let handle = thread::spawn(move || {
			let _panic_handler = PanicHandler { handler: panic_handler };

			// TODO [ToDr] Errors?
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
			})).expect("Cannot bind to socket.");

			let local_addr = server.local_addr().expect("OK");
			// Add current host to allowed headers.
			// NOTE: we need to use `local_address` instead of `addr`
			// it might be different!
			*hosts_setter = Self::update_hosts(allowed_hosts, local_addr.clone());
			drop(hosts_setter);

			// Send local address
			local_addr_tx.send(local_addr).expect("Server initialization awaits local address.");

			// Start the server and wait for shutdown signal
			server.run_until(shutdown_signal.map_err(|_| {
				warn!("Shutdown signaller dropped, closing server.");
			})).expect("Expected clean shutdown.")
		});

		// Wait for server initialization
		let local_addr = local_addr_rx.recv().map_err(|_| {
			RpcServerError::IoError(io::Error::new(io::ErrorKind::Interrupted, ""))
		})?;

		Ok(Server {
			address: local_addr,
			handle: Some(handle),
			close: Some(close),
		})
	}
}

struct PanicHandler {
	handler: Option<Box<Fn() -> () + Send>>,
}

impl Drop for PanicHandler {
	fn drop(&mut self) {
		if ::std::thread::panicking() {
			if let Some(ref h) = self.handler {
				h();
			}
		}
	}
}

/// Tokio-proto JSON-RPC HTTP Service
pub struct RpcService<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<HttpMetaExtractor<M>>,
	hosts: Option<Vec<String>>,
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
	// TODO avoid boxing here
	type Future = BoxFuture<tokio_minihttp::Response, io::Error>;

	fn call(&self, request: Self::Request) -> Self::Future {
		let request = req::Req::new(request);
		// Validate HTTP Method
		let is_options = request.method() == req::Method::Options;
		if !is_options && request.method() != req::Method::Post {
			return future::ok(
				res::method_not_allowed()
			).boxed();
		}

		// Validate allowed hosts
		let host = request.header("Host");
		if !hosts::is_host_valid(host, &self.hosts) {
			return future::ok(
				res::invalid_host()
			).boxed();
		}

		// Validate content type
		let content_type = request.header("Content-type");
		if !is_json(content_type) {
			return future::ok(
				res::invalid_content_type()
			).boxed();
		}

		// Extract CORS headers
		let origin = request.header("Origin");
		let cors = cors::get_cors_header(origin, &self.cors_domains);

		// Don't process data if it's OPTIONS
		if is_options {
			return future::ok(
				res::new("", cors)
			).boxed();
		}

		// Extract metadata
		let metadata = self.meta_extractor.read_metadata(&request);

		// Read & handle request
		let data = request.body();
		self.handler.handle_request(data, metadata).map(|result| {
			let result = format!("{}\n", result.unwrap_or_default());
			res::new(&result, cors)
		}).map_err(|_| unimplemented!()).boxed()
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
		self.close.take().map(|close| close.complete(()));
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.close.take().map(|close| close.complete(()));
	}
}
