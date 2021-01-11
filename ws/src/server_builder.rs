use std::net::SocketAddr;
use std::sync::Arc;

use crate::core;
use crate::server_utils::cors::Origin;
use crate::server_utils::hosts::{DomainsValidation, Host};
use crate::server_utils::reactor::{self, UninitializedExecutor};
use crate::server_utils::session::SessionStats;

use crate::error::Result;
use crate::metadata::{MetaExtractor, NoopExtractor};
use crate::server::Server;
use crate::session;

/// Builder for `WebSockets` server
pub struct ServerBuilder<M: core::Metadata, S: core::Middleware<M>> {
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	allowed_hosts: Option<Vec<Host>>,
	request_middleware: Option<Arc<dyn session::RequestMiddleware>>,
	session_stats: Option<Arc<dyn SessionStats>>,
	executor: UninitializedExecutor,
	max_connections: usize,
	max_payload_bytes: usize,
	max_in_buffer_capacity: usize,
	max_out_buffer_capacity: usize,
}

impl<M: core::Metadata + Default, S: core::Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new `ServerBuilder`
	pub fn new<T>(handler: T) -> Self
	where
		T: Into<core::MetaIoHandler<M, S>>,
	{
		Self::with_meta_extractor(handler, NoopExtractor)
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new `ServerBuilder`
	pub fn with_meta_extractor<T, E>(handler: T, extractor: E) -> Self
	where
		T: Into<core::MetaIoHandler<M, S>>,
		E: MetaExtractor<M>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(extractor),
			allowed_origins: None,
			allowed_hosts: None,
			request_middleware: None,
			session_stats: None,
			executor: UninitializedExecutor::Unspawned,
			max_connections: 100,
			max_payload_bytes: 5 * 1024 * 1024,
			max_in_buffer_capacity: 10 * 1024 * 1024,
			max_out_buffer_capacity: 10 * 1024 * 1024,
		}
	}

	/// Utilize existing event loop executor to poll RPC results.
	pub fn event_loop_executor(mut self, executor: reactor::TaskExecutor) -> Self {
		self.executor = UninitializedExecutor::Shared(executor);
		self
	}

	/// Sets a meta extractor.
	pub fn session_meta_extractor<T: MetaExtractor<M>>(mut self, extractor: T) -> Self {
		self.meta_extractor = Arc::new(extractor);
		self
	}

	/// Allowed origins.
	pub fn allowed_origins(mut self, allowed_origins: DomainsValidation<Origin>) -> Self {
		self.allowed_origins = allowed_origins.into();
		self
	}

	/// Allowed hosts.
	pub fn allowed_hosts(mut self, allowed_hosts: DomainsValidation<Host>) -> Self {
		self.allowed_hosts = allowed_hosts.into();
		self
	}

	/// Session stats
	pub fn session_stats<T: SessionStats>(mut self, stats: T) -> Self {
		self.session_stats = Some(Arc::new(stats));
		self
	}

	/// Sets a request middleware. Middleware will be invoked before each handshake request.
	/// You can either terminate the handshake in the middleware or run a default behaviour after.
	pub fn request_middleware<T: session::RequestMiddleware>(mut self, middleware: T) -> Self {
		self.request_middleware = Some(Arc::new(middleware));
		self
	}

	/// Maximal number of concurrent connections this server supports.
	/// Default: 100
	pub fn max_connections(mut self, max_connections: usize) -> Self {
		self.max_connections = max_connections;
		self
	}

	/// Maximal size of the payload (in bytes)
	/// Default: 5MB
	pub fn max_payload(mut self, max_payload_bytes: usize) -> Self {
		self.max_payload_bytes = max_payload_bytes;
		self
	}

	/// The maximum size to which the incoming buffer can grow.
	/// Default: 10,485,760
	pub fn max_in_buffer_capacity(mut self, max_in_buffer_capacity: usize) -> Self {
		self.max_in_buffer_capacity = max_in_buffer_capacity;
		self
	}

	/// The maximum size to which the outgoing buffer can grow.
	/// Default: 10,485,760
	pub fn max_out_buffer_capacity(mut self, max_out_buffer_capacity: usize) -> Self {
		self.max_out_buffer_capacity = max_out_buffer_capacity;
		self
	}

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start(self, addr: &SocketAddr) -> Result<Server> {
		Server::start(
			addr,
			self.handler,
			self.meta_extractor,
			self.allowed_origins,
			self.allowed_hosts,
			self.request_middleware,
			self.session_stats,
			self.executor,
			self.max_connections,
			self.max_payload_bytes,
			self.max_in_buffer_capacity,
			self.max_out_buffer_capacity,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn basic_server_builder() -> ServerBuilder<(), jsonrpc_core::middleware::Noop> {
		let io = core::IoHandler::default();
		ServerBuilder::new(io)
	}
	#[test]
	fn config_usize_vals_have_correct_defaults() {
		let server = basic_server_builder();

		assert_eq!(server.max_connections, 100);
		assert_eq!(server.max_payload_bytes, 5 * 1024 * 1024);
		assert_eq!(server.max_in_buffer_capacity, 10 * 1024 * 1024);
		assert_eq!(server.max_out_buffer_capacity, 10 * 1024 * 1024);
	}

	#[test]
	fn config_usize_vals_can_be_set() {
		let server = basic_server_builder();

		// We can set them individually
		let server = server.max_connections(10);
		assert_eq!(server.max_connections, 10);

		let server = server.max_payload(29);
		assert_eq!(server.max_payload_bytes, 29);

		let server = server.max_in_buffer_capacity(38);
		assert_eq!(server.max_in_buffer_capacity, 38);

		let server = server.max_out_buffer_capacity(47);
		assert_eq!(server.max_out_buffer_capacity, 47);

		// Setting values consecutively does not impact other values
		assert_eq!(server.max_connections, 10);
		assert_eq!(server.max_payload_bytes, 29);
		assert_eq!(server.max_in_buffer_capacity, 38);
		assert_eq!(server.max_out_buffer_capacity, 47);
	}
}
