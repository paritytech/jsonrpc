use std::sync::Arc;
use std::net::SocketAddr;

use futures::BoxFuture;
use tokio_service;

use jsonrpc::{Metadata, MetaIoHandler};

#[derive(Clone)]
pub struct SocketMetadata {
    addr: SocketAddr,
}

impl Default for SocketMetadata {
    fn default() -> Self {
        SocketMetadata { addr: "0.0.0.0:0".parse().unwrap() }
    }
}

impl Metadata for SocketMetadata { }

impl From<SocketAddr> for SocketMetadata {
    fn from(addr: SocketAddr) -> SocketMetadata {
        SocketMetadata { addr: addr }
    }
}

pub struct Service<M: Metadata> {
    handler: Arc<MetaIoHandler<M>>,
    peer_addr: SocketAddr,
    meta: M,
}

impl<M: Metadata> Service<M> {
    pub fn new(peer_addr: SocketAddr, handler: Arc<MetaIoHandler<M>>) -> Self {
        Service { peer_addr: peer_addr, handler: handler, meta: M::default() }
    }
}

impl<M: Metadata> tokio_service::Service for Service<M> {
    // These types must match the corresponding protocol types:
    type Request = String;
    type Response = Option<String>;

    // For non-streaming protocols, service errors are always io::Error
    type Error = ();

    // The future for computing the response; box it for simplicity.
    type Future = BoxFuture<Self::Response, Self::Error>;

    // Produce a future for computing a response from a request.
    fn call(&self, req: Self::Request) -> Self::Future {
        trace!(target: "tcp", "Accepted request from peer {}: {}", &self.peer_addr, req);
        self.handler.handle_request(&req, self.meta.clone())
    }
}
