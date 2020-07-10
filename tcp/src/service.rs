use std::net::SocketAddr;
use std::sync::Arc;

use crate::jsonrpc::{middleware, MetaIoHandler, Metadata, Middleware};
use crate::jsonrpc::futures::FutureExt;
use futures::Future;

pub struct Service<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	peer_addr: SocketAddr,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
	pub fn new(peer_addr: SocketAddr, handler: Arc<MetaIoHandler<M, S>>, meta: M) -> Self {
		Service {
			peer_addr,
			handler,
			meta,
		}
	}
}

impl<M: Metadata, S: Middleware<M>> tokio_service::Service for Service<M, S> where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	// These types must match the corresponding protocol types:
	type Request = String;
	type Response = Option<String>;

	// For non-streaming protocols, service errors are always io::Error
	type Error = ();

	// The future for computing the response; box it for simplicity.
	type Future = Box<dyn Future<Item = Option<String>, Error = ()> + Send>;

	// Produce a future for computing a response from a request.
	fn call(&self, req: Self::Request) -> Self::Future {
		trace!(target: "tcp", "Accepted request from peer {}: {}", &self.peer_addr, req);
		Box::new(futures03::compat::Compat::new(
			self.handler.handle_request(&req, self.meta.clone()).map(|v| Ok(v))
		))
	}
}
