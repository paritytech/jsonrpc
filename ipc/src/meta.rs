use crate::jsonrpc::futures::sync::mpsc;
use crate::jsonrpc::Metadata;
use crate::server_utils::session;

/// Request context
pub struct RequestContext<'a> {
	/// Session ID
	pub session_id: session::SessionId,
	/// Remote UDS endpoint
	pub endpoint_addr: &'a ::parity_tokio_ipc::RemoteId,
	/// Direct pipe sender
	pub sender: mpsc::Sender<String>,
}

/// Metadata extractor (per session)
pub trait MetaExtractor<M: Metadata> : Send + Sync + 'static {
	/// Extracts metadata from request context
	fn extract(&self, context: &RequestContext<'_>) -> M;
}

impl<M, F> MetaExtractor<M> for F where
	M: Metadata,
	F: Fn(&RequestContext<'_>) -> M + Send + Sync + 'static,
{
	fn extract(&self, context: &RequestContext<'_>) -> M {
		(*self)(context)
	}
}

/// Noop-extractor
pub struct NoopExtractor;
impl<M: Metadata + Default> MetaExtractor<M> for NoopExtractor {
	fn extract(&self, _context: &RequestContext<'_>) -> M { M::default() }
}
