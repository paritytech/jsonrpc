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
extern crate futures;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_minihttp;

// pub mod request_response;
// pub mod cors;
// mod handler;
// mod hosts_validator;
#[cfg(test)]
mod tests;

use std::io;
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::thread;
use std::collections::HashSet;
use std::ops::Deref;
use futures::{future, Future, BoxFuture};
use jsonrpc::MetaIoHandler;
use jsonrpc::reactor::{RpcHandler, RpcEventLoop, RpcEventLoopHandle};

// pub use handler::{PanicHandler, ServerHandler};
// pub use hosts_validator::is_host_header_valid;


pub enum AccessControlAllowOrigin {
	Value(String),
	Null,
	All,
}

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
//
// /// Extracts metadata from the HTTP request.
// pub trait HttpMetaExtractor<M: jsonrpc::Metadata>: Sync + Send + 'static {
// 	/// Read the metadata from the request
// 	fn read_metadata(&self, _: &server::Request<hyper::net::HttpStream>) -> M {
// 		Default::default()
// 	}
// }
//
// #[derive(Default)]
// struct NoopExtractor;
// impl<M: jsonrpc::Metadata> HttpMetaExtractor<M> for NoopExtractor {}

/// RPC Handler bundled with metadata extractor.
pub struct Rpc<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	/// RPC Handler
	pub handler: RpcHandler<M, S>,
	// / Metadata extractor
	// pub extractor: Arc<HttpMetaExtractor<M>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Clone for Rpc<M, S> {
	fn clone(&self) -> Self {
		Rpc {
			handler: self.handler.clone(),
			// extractor: self.extractor.clone(),
		}
	}
}
//
// impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Rpc<M, S> {
// 	/// Creates new RPC with extractor
// 	pub fn new(handler: RpcHandler<M, S>, extractor: Arc<HttpMetaExtractor<M>>) -> Self {
// 		Rpc {
// 			handler: handler,
// 			// extractor: extractor,
// 		}
// 	}
// }

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> From<RpcHandler<M, S>> for Rpc<M, S> {
	fn from(handler: RpcHandler<M, S>) -> Self {
		Rpc {
			handler: handler,
			// extractor: Arc::new(NoopExtractor),
		}
	}
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Deref for Rpc<M, S> {
	type Target = RpcHandler<M, S>;

	fn deref(&self) -> &Self::Target {
		&self.handler
	}
}

/// Convenient JSON-RPC HTTP Server builder.
pub struct ServerBuilder<M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	jsonrpc_handler: Arc<MetaIoHandler<M, S>>,
	// meta_extractor: Arc<HttpMetaExtractor<M>>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
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
			jsonrpc_handler: Arc::new(handler.into()),
			// meta_extractor: Arc::new(NoopExtractor::default()),
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
    //
	// /// Configures metadata extractor
	// pub fn meta_extractor(mut self, extractor: Arc<HttpMetaExtractor<M>>) -> Self {
	// 	self.meta_extractor = extractor;
	// 	self
	// }

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


		// let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |control| {
		// 	let hosts = hosts.lock().unwrap().clone();
		// 	ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), hosts, handler, control)
		// }));
        //
		// // Add current host to allowed headers.
		// // NOTE: we need to use `l.addrs()` instead of `addr`
		// // it might be different!
		// {
		// 	let mut hosts = hosts_setter.lock().unwrap();
		// 	if let Some(current_hosts) = hosts.take() {
		// 		let mut new_hosts = current_hosts.into_iter().collect::<HashSet<_>>();
		// 		for addr in l.addrs() {
		// 			let address = addr.to_string();
		// 			new_hosts.insert(address.clone());
		// 			new_hosts.insert(address.replace("127.0.0.1", "localhost"));
		// 		}
		// 		// Override hosts
		// 		*hosts = Some(new_hosts.into_iter().collect());
		// 	}
		// }
		let mut addr = addr.clone();
		if addr.port() == 0 {
			addr.set_port(6000);
		}

		let a = addr.clone();
		let handler = self.jsonrpc_handler.clone();
		let handle = thread::spawn(move || {
			let server = tokio_proto::TcpServer::new(tokio_minihttp::Http, a);
			server.serve(move || Ok(RpcService {
				handler: handler.clone(),
			}))
		});

		Ok(Server {
			addr: [addr],
			handle: Some(handle),
			// event_loop: event_loop,
		})
	}
}

pub struct RpcService<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> {
	handler: Arc<MetaIoHandler<M, S>>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> tokio_service::Service for RpcService<M, S> {
	type Request = tokio_minihttp::Request;
	type Response = tokio_minihttp::Response;
	type Error = io::Error;
	type Future = BoxFuture<tokio_minihttp::Response, io::Error>;

	fn call(&self, request: Self::Request) -> Self::Future {
		let body = request.body();
		let data = ::std::str::from_utf8(body.as_slice()).ok().unwrap_or("");
		self.handler.handle_request(data, Default::default()).map(|result| {
			let mut resp = Self::Response::new();
			resp.body(&result.unwrap_or_default());
			resp
		}).map_err(|_| unimplemented!()).boxed()
	}
}


/// jsonrpc http server instance
pub struct Server {
	// server: Option<server::Listening>,
	addr: [SocketAddr; 1],
	handle: Option<thread::JoinHandle<()>>,
	// event_loop: Option<RpcEventLoopHandle>,
}

impl Server {
	/// Returns addresses of this server
	pub fn addrs(&self) -> &[SocketAddr] {
		&self.addr
	}

	/// Closes the server.
	pub fn close(mut self) {
		// self.server.take().unwrap().close();
		// self.event_loop.take().map(|v| v.close());
	}

	/// Will block, waiting for the server to finish.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		// self.server.take().unwrap().close();
		// self.event_loop.take().map(|v| v.close());
	}
}
