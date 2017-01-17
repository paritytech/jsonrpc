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

pub struct Service {
    handler: Arc<MetaIoHandler<SocketMetadata>>,
    peer_addr: SocketAddr,
}

impl Service {
    pub fn new(peer_addr: SocketAddr, handler: Arc<MetaIoHandler<SocketMetadata>>) -> Self {
        Service { peer_addr: peer_addr, handler: handler }
    }
}

impl tokio_service::Service for Service {
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
        let meta: SocketMetadata = self.peer_addr.into();
        self.handler.handle_request(&req, meta)
    }
}
