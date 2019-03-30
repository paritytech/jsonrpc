//! `WebSockets` server.

#![warn(missing_docs)]

use jsonrpc_server_utils as server_utils;

pub use jsonrpc_core;
pub use ws;

#[macro_use]
extern crate log;

mod error;
mod metadata;
mod server;
mod server_builder;
mod session;
#[cfg(test)]
mod tests;

use jsonrpc_core as core;

pub use self::error::{Error, Result};
pub use self::metadata::{MetaExtractor, NoopExtractor, RequestContext};
pub use self::server::{Broadcaster, CloseHandle, Server};
pub use self::server_builder::ServerBuilder;
pub use self::server_utils::cors::Origin;
pub use self::server_utils::hosts::{DomainsValidation, Host};
pub use self::server_utils::session::{SessionId, SessionStats};
pub use self::server_utils::tokio;
pub use self::session::{MiddlewareAction, RequestMiddleware};
