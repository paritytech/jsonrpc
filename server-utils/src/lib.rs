//! JSON-RPC servers utilities.

#![deny(missing_docs)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

use jsonrpc_core as core;

pub use tokio;
pub use tokio_codec;

pub mod cors;
pub mod hosts;
mod matcher;
pub mod reactor;
pub mod session;
mod stream_codec;
mod suspendable_stream;

pub use crate::matcher::Pattern;
pub use crate::suspendable_stream::SuspendableStream;

/// Codecs utilities
pub mod codecs {
	pub use crate::stream_codec::{Separator, StreamCodec};
}
