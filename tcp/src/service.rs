use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::futures;
use crate::jsonrpc::{middleware, MetaIoHandler, Metadata, Middleware};

pub struct Service<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	peer_addr: SocketAddr,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
	pub fn new(peer_addr: SocketAddr, handler: Arc<MetaIoHandler<M, S>>, meta: M) -> Self {
		Service {
			handler,
			peer_addr,
			meta,
		}
	}
}

impl<M: Metadata, S: Middleware<M>> tower_service::Service<String> for Service<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	// These types must match the corresponding protocol types:
	type Response = Option<String>;
	// For non-streaming protocols, service errors are always io::Error
	type Error = ();

	// The future for computing the response; box it for simplicity.
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	// Produce a future for computing a response from a request.
	fn call(&mut self, req: String) -> Self::Future {
		use futures::FutureExt;
		trace!(target: "tcp", "Accepted request from peer {}: {}", &self.peer_addr, req);
		Box::pin(self.handler.handle_request(&req, self.meta.clone()).map(Ok))
	}
}
