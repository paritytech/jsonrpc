//! `WebSockets` server.

#![warn(missing_docs)]

extern crate jsonrpc_core as core;
extern crate ws;

#[macro_use]
extern crate log;

mod metadata;
mod server;
mod server_builder;
mod session;
#[cfg(test)]
mod tests;

pub use self::metadata::{RequestContext, MetaExtractor, NoopExtractor};
pub use self::server::Server;
pub use self::server_builder::{ServerBuilder, ServerError};


