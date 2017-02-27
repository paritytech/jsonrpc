//! `WebSockets` server.

extern crate jsonrpc_core as core;
extern crate ws;

#[macro_use]
extern crate log;

mod metadata;
mod server;
mod session;

use std::net::SocketAddr;
use std::sync::Arc;

pub use self::metadata::{RequestContext, MetaExtractor, NoopExtractor};
pub use self::server::Server;

/// Signer startup error
#[derive(Debug)]
pub enum ServerError {
	/// Wrapped `std::io::Error`
	IoError(std::io::Error),
	/// Other `ws-rs` error
	WebSocket(ws::Error)
}

impl From<ws::Error> for ServerError {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(e) => ServerError::IoError(e),
			_ => ServerError::WebSocket(err),
		}
	}
}

/// Builder for `WebSockets` server
pub struct ServerBuilder<M: core::Metadata, S: core::Middleware<M>> {
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
}

impl<M: core::Metadata, S: core::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder`
	pub fn new<T>(handler: T) -> Self where
		T: Into<core::MetaIoHandler<M, S>>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			allowed_origins: None,
		}
	}

	/// Sets a meta extractor.
	pub fn session_meta_extractor<T: MetaExtractor<M>>(mut self, extractor: T) -> Self {
		self.meta_extractor = Arc::new(extractor);
		self
	}

	/// Allowed origins.
	pub fn allowed_origins(mut self, allowed_origins: Option<Vec<String>>) -> Self {
		self.allowed_origins = allowed_origins;
		self
	}

	// TODO [ToDr] Session statistics
	// TODO [ToDr] Connection middleware

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start(self, addr: &SocketAddr) -> Result<Server, ServerError> {
		Server::start(
			addr,
			self.handler,
			self.meta_extractor,
			self.allowed_origins,
		)
	}

}


