extern crate jsonrpc_core as jsonrpc;
extern crate tokio_service;
extern crate tokio_uds;

use std::sync::Arc;
use std::net::SocketAddr;

use self::jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use self::jsonrpc::futures::BoxFuture;

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
