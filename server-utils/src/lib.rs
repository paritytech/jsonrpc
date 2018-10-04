//! JSON-RPC servers utilities.

#![warn(missing_docs)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

extern crate globset;
extern crate jsonrpc_core as core;
extern crate bytes;
extern crate num_cpus;

pub extern crate tokio;
pub extern crate tokio_codec;

pub mod cors;
pub mod hosts;
pub mod session;
pub mod reactor;
mod matcher;
mod stream_codec;
mod suspendable_stream;

pub use suspendable_stream::SuspendableStream;
pub use matcher::Pattern;

/// Codecs utilities
pub mod codecs {
    pub use stream_codec::{StreamCodec, Separator};
}

