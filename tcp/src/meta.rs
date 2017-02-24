use std::net::SocketAddr;
use jsonrpc::Metadata;

pub struct RequestContext {
	pub peer_addr: SocketAddr,
}

pub trait MetaExtractor<M: Metadata> : Send + Sync {
	fn extract(&self, context: &RequestContext) -> M;
}

pub struct NoopExtractor;

impl<M: Metadata> MetaExtractor<M> for NoopExtractor {
	fn extract(&self, _context: &RequestContext) -> M { M::default() }
}
