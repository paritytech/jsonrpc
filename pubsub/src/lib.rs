//! Publish-Subscribe extension for JSON-RPC

#![warn(missing_docs)]

extern crate jsonrpc_core as core;
extern crate parking_lot;

#[macro_use]
extern crate log;

mod handler;
mod subscription;
mod types;

pub use self::handler::{PubSubHandler, SubscribeRpcMethod, UnsubscribeRpcMethod};
pub use self::subscription::{Session, Sink, Subscriber, new_subscription};
pub use self::types::{PubSubMetadata, SubscriptionId, TransportError, SinkResult};
