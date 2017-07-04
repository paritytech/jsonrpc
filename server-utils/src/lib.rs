//! JSON-RPC servers utilities.

#![warn(missing_docs)]

#[macro_use]
extern crate log;

extern crate globset;
extern crate jsonrpc_core as core;
extern crate bytes;

pub extern crate tokio_core;
pub extern crate tokio_io;

pub mod cors;
pub mod hosts;
pub mod session;
pub mod reactor;
mod matcher;
mod stream_codec;

pub use matcher::Pattern;

/// Codecs utilities
pub mod codecs {
    pub use stream_codec::{StreamCodec, Separator};
}

