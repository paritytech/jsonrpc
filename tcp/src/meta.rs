use std::net::SocketAddr;

use jsonrpc::futures::sync::mpsc;
use jsonrpc::Metadata;

pub struct RequestContext {
	pub peer_addr: SocketAddr,
	pub sender: mpsc::Sender<String>,
}

pub trait MetaExtractor<M: Metadata> : Send + Sync {
	fn extract(&self, context: &RequestContext) -> M;
}

impl<M, F> MetaExtractor<M> for F where
	M: Metadata,
	F: Fn(&RequestContext) -> M + Send + Sync,
{
	fn extract(&self, context: &RequestContext) -> M {
		(*self)(context)
	}
}

pub struct NoopExtractor;
impl<M: Metadata> MetaExtractor<M> for NoopExtractor {
	fn extract(&self, _context: &RequestContext) -> M { M::default() }
}
