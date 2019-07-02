//! Publish-Subscribe extension for JSON-RPC

#![deny(missing_docs)]

use jsonrpc_core as core;

#[macro_use]
extern crate log;

mod delegates;
mod handler;
pub mod oneshot;
mod subscription;
pub mod typed;
mod types;

pub use self::delegates::IoDelegate;
pub use self::handler::{PubSubHandler, SubscribeRpcMethod, UnsubscribeRpcMethod};
pub use self::subscription::{new_subscription, Session, Sink, Subscriber};
pub use self::types::{PubSubMetadata, SinkResult, SubscriptionId, TransportError};
