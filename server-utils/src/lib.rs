//! JSON-RPC servers utilities.

#![deny(missing_docs)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

#[cfg(feature = "tokio")]
pub use tokio;
pub use tokio_util;
#[cfg(feature = "tokio-compat")]
pub use tokio_compat;
#[cfg(feature = "tokio02")]
pub use tokio02;
#[cfg(feature = "tokio-codec")]
pub use tokio_codec;

pub mod cors;
pub mod hosts;
mod matcher;
pub mod reactor;
pub mod session;
mod stream_codec;
#[cfg(feature = "tokio")]
mod suspendable_stream;

pub use crate::matcher::Pattern;
#[cfg(feature = "tokio")]
pub use crate::suspendable_stream::SuspendableStream;

/// Codecs utilities
pub mod codecs {
	pub use crate::stream_codec::{Separator, StreamCodec};
}
