//! Cross-platform JSON-RPC IPC transport.

#![deny(missing_docs)]

use jsonrpc_server_utils as server_utils;

pub use jsonrpc_core;

#[macro_use]
extern crate log;

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[cfg(test)]
mod logger;

mod meta;
mod select_with_weak;
mod server;

use jsonrpc_core as jsonrpc;

pub use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
pub use crate::server::{CloseHandle, SecurityAttributes, Server, ServerBuilder};

pub use self::server_utils::session::{SessionId, SessionStats};
pub use self::server_utils::{codecs::Separator, tokio};
