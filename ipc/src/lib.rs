//! Cross-platform JSON-RPC IPC transport.

#![warn(missing_docs)]

extern crate jsonrpc_server_utils as server_utils;
extern crate parity_tokio_ipc;
extern crate tokio_service;

pub extern crate jsonrpc_core;

#[macro_use] extern crate log;

#[cfg(test)] #[macro_use] extern crate lazy_static;
#[cfg(test)] extern crate env_logger;
#[cfg(test)] mod logger;

mod server;
mod select_with_weak;
mod meta;

use jsonrpc_core as jsonrpc;

pub use meta::{MetaExtractor, RequestContext};
pub use server::{Server, ServerBuilder};

pub use self::server_utils::tokio_core;
pub use self::server_utils::session::{SessionStats, SessionId};
