//! Cross-platform JSON-RPC IPC transport.

#![warn(missing_docs)]

extern crate jsonrpc_core as jsonrpc;
extern crate jsonrpc_server_utils as server_utils;
extern crate parity_tokio_ipc;
extern crate tokio_service;
extern crate bytes;

#[macro_use] extern crate log;

#[cfg(test)] #[macro_use] extern crate lazy_static;
#[cfg(test)] extern crate env_logger;
#[cfg(test)] mod logger;

mod stream_codec;
mod server;
mod meta;
pub use meta::{MetaExtractor, RequestContext};
pub use server::{Server, ServerBuilder};

pub use self::server_utils::tokio_core;
