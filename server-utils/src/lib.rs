//! JSON-RPC servers utilities.

#![deny(missing_docs)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

pub use tokio_util;
#[cfg(feature = "tokio02")]
pub use tokio02;

pub mod cors;
pub mod hosts;
mod matcher;
pub mod reactor;
pub mod session;
mod stream_codec;
#[cfg(feature = "tokio02")]
mod suspendable_stream;

pub use crate::matcher::Pattern;
#[cfg(feature = "tokio02")]
pub use crate::suspendable_stream::SuspendableStream;

/// Codecs utilities
pub mod codecs {
	pub use crate::stream_codec::{Separator, StreamCodec};
}
