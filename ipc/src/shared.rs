use std::io;

use jsonrpc_core::{Metadata, MetaIoHandler, Middleware};
use jsonrpc_server_utils::tokio_core::reactor::Remote;

/// Request context
pub struct RequestContext;

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

/// Server result.
pub type Result<T> = ::std::result::Result<T, Error>;

/// Server error
#[derive(Debug)]
pub enum Error {
	/// IO error
	Io(io::Error),
	/// Server is not started yet
	NotStarted,
	/// Server is already stopping
	AlreadyStopping,
	/// Server is not stopped yet
	NotStopped,
	/// Server is stopping
	IsStopping,
}

impl From<io::Error> for Error {
	fn from(io_error: io::Error) -> Error {
		Error::Io(io_error)
	}
}

/// Cross-platform IpcServer trait.
pub trait IpcServer<M: Metadata, S: Middleware<M>>: Sized {
	/// Starts a new server asynchronously.
	fn start<I, E>(
		io: I,
		path: &str,
		remote: Remote,
		extractor: E,
	) -> Result<Self> where
		I: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M>;
}
