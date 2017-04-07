use core;
use ws;

use session;

/// Request context
pub struct RequestContext {
	/// Session id
	pub session_id: session::SessionId,
	/// Direct channel to send messages to a client.
	pub out: ws::Sender,
}

/// Metadata extractor from session data.
pub trait MetaExtractor<M: core::Metadata>: Send + Sync + 'static {
	/// Extract metadata for given session
	fn extract(&self, _context: &RequestContext) -> M {
		Default::default()
	}
}

/// Dummy metadata extractor
#[derive(Clone)]
pub struct NoopExtractor;
impl<M: core::Metadata> MetaExtractor<M> for NoopExtractor {}
