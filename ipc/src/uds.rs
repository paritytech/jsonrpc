extern crate tokio_uds;
extern crate tokio_service;

extern crate jsonrpc_core as jsonrpc;
extern crate jsonrpc_server_utils as server_utils;

use std;
use std::sync::{Arc, Mutex};

use self::tokio_service::Service as TokioService;
use self::jsonrpc::futures::{future, Future, Stream, Sink};
use self::jsonrpc::futures::sync::{mpsc, oneshot};
use self::jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use self::jsonrpc::futures::BoxFuture;
use self::server_utils::tokio_core::io::Io;
use self::server_utils::{reactor, tokio_core};

pub struct Service<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
	pub fn new(handler: Arc<MetaIoHandler<M, S>>, meta: M) -> Self {
		Service { handler: handler, meta: meta }
	}
}

impl<M: Metadata, S: Middleware<M>> tokio_service::Service for Service<M, S> {
	type Request = String;
	type Response = Option<String>;

	type Error = ();

	type Future = BoxFuture<Self::Response, Self::Error>;

	fn call(&self, req: Self::Request) -> Self::Future {
		trace!(target: "tcp", "Accepted request: {}", req);
		self.handler.handle_request(&req, self.meta.clone())
	}
}

pub struct ServerBuilder<M: Metadata, S: Middleware<M>> {
	addr: String,
	remote: reactor::UnitializedRemote,
	handler: Arc<MetaIoHandler<M, S>>,
}

/// IPC Server handle
pub struct Server {
	remote: Option<reactor::Remote>,
	stop: Option<oneshot::Sender<()>>,
}

#[derive(Debug)]
pub enum Error {
	Io(std::io::Error),
}

impl std::convert::From<std::io::Error> for Error {
	fn from(io_error: std::io::Error) -> Error {
		Error::Io(io_error)
	}
}

impl<M: Metadata, S: Middleware<M> + Send + Sync + 'static> ServerBuilder<M, S> {
	pub fn new<T>(socket_addr: &str, io_handler: T) -> Result<ServerBuilder<M, S>, Error> where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_remote(
			socket_addr,
			io_handler,
			reactor::UnitializedRemote::Unspawned,
		)
	}

	pub fn with_remote<T>(
		socket_addr: &str,
		io_handler: T,
		remote: reactor::UnitializedRemote,
	) -> Result<ServerBuilder<M, S>, Error> 
		where T: Into<MetaIoHandler<M, S>>,
	{
		Ok(ServerBuilder {
			addr: socket_addr.to_owned(),
			remote: remote,
			handler: Arc::new(io_handler.into()),
		})
	}	

	/// Run server (in separate thread)
	pub fn run_async(self) -> Result<Server, Error> {
		let remote = self.remote.initialize()?;
		remote.remote().spawn(move |handle| {
			
		});
		Ok(())			
	}	
}