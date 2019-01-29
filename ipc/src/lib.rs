//! Cross-platform JSON-RPC IPC transport.

#![warn(missing_docs)]

use jsonrpc_server_utils as server_utils;
use tokio_service;

pub use jsonrpc_core;

#[macro_use] extern crate log;

#[cfg(test)] #[macro_use] extern crate lazy_static;

#[cfg(test)] mod logger;

mod server;
mod select_with_weak;
mod meta;

use jsonrpc_core as jsonrpc;

pub use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
pub use crate::server::{Server, ServerBuilder, CloseHandle,SecurityAttributes};

pub use self::server_utils::{tokio, codecs::Separator};
pub use self::server_utils::session::{SessionStats, SessionId};
