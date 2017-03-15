use jsonrpc::Metadata;

/// Request context
pub struct RequestContext<'a> {
	/// Remote UDS endpoint
	pub endpoint_addr: &'a ::parity_tokio_ipc::RemoteId,
}

/// Metadata extractor (per session)
pub trait MetaExtractor<M: Metadata> : Send + Sync + 'static {
	/// Extracts metadata from request context
	fn extract(&self, context: &RequestContext) -> M;
}

impl<M, F> MetaExtractor<M> for F where
	M: Metadata,
	F: Fn(&RequestContext) -> M + Send + Sync + 'static,
{
	fn extract(&self, context: &RequestContext) -> M {
		(*self)(context)
	}
}

/// Noop-extractor
pub struct NoopExtractor;
impl<M: Metadata> MetaExtractor<M> for NoopExtractor {
	fn extract(&self, _context: &RequestContext) -> M { M::default() }
}
