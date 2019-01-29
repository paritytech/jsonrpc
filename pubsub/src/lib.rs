//! Publish-Subscribe extension for JSON-RPC

#![warn(missing_docs)]

use jsonrpc_core as core;

#[macro_use]
extern crate log;

mod delegates;
mod handler;
mod subscription;
mod types;
pub mod typed;

pub use self::handler::{PubSubHandler, SubscribeRpcMethod, UnsubscribeRpcMethod};
pub use self::delegates::IoDelegate;
pub use self::subscription::{Session, Sink, Subscriber, new_subscription};
pub use self::types::{PubSubMetadata, SubscriptionId, TransportError, SinkResult};
