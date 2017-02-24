use std::sync::Arc;
use core;
use core::futures::sync::mpsc;

use subscription::Session;

/// Raw transport sink for specific client.
pub type TransportSender = mpsc::Sender<String>;

/// Metadata extension for pub-sub method handling.
pub trait PubSubMetadata: core::Metadata {
	/// Returns session object associated with given request/client.
	/// `None` indicates that sessions are not supported on the used transport.
	fn session(&self) -> Option<Arc<Session>>;
}

/// Unique subscription id.
/// NOTE Assigning same id to different requests will cause the previous request to be unsubscribed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionId {
	/// U64 number
	Number(u64),
	/// String
	String(String),
}

impl SubscriptionId {
	/// Parses `core::Value` into unique subscription id.
	pub fn parse_value(val: &core::Value) -> Option<SubscriptionId> {
		match *val {
			core::Value::String(ref val) => Some(SubscriptionId::String(val.clone())),
			core::Value::Number(ref val) => val.as_u64().map(SubscriptionId::Number),
			_ => None,
		}
	}
}

impl From<SubscriptionId> for core::Value {
	fn from(sub: SubscriptionId) -> Self {
		match sub {
			SubscriptionId::Number(val) => core::Value::Number(val.into()),
			SubscriptionId::String(val) => core::Value::String(val),
		}
	}
}

