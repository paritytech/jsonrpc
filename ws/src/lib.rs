//! `WebSockets` server.

#![warn(missing_docs)]

#[macro_use]
extern crate log;
extern crate jsonrpc_core as core;
extern crate jsonrpc_server_utils as server_utils;
pub extern crate ws;

mod metadata;
mod server;
mod server_builder;
mod session;
#[cfg(test)]
mod tests;

pub use self::metadata::{RequestContext, MetaExtractor, NoopExtractor};
pub use self::server::Server;
pub use self::session::{SessionStats, SessionId, RequestMiddleware, MiddlewareAction};
pub use self::server_builder::{ServerBuilder, ServerError};
pub use self::server_utils::tokio_core;

