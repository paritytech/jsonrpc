use std::sync::Arc;
use core;
use core::futures::sync::mpsc;

use subscription::Session;

/// Raw transport sink for specific client.
pub type TransportSender = mpsc::Sender<String>;
/// Raw transport error.
pub type TransportError = mpsc::SendError<String>;
/// Subscription send result.
pub type SinkResult = core::futures::sink::Send<TransportSender>;

/// Metadata extension for pub-sub method handling.
pub trait PubSubMetadata: core::Metadata {
	/// Returns session object associated with given request/client.
	/// `None` indicates that sessions are not supported on the used transport.
	fn session(&self) -> Option<Arc<Session>>;
}

impl PubSubMetadata for Arc<Session> {
	fn session(&self) -> Option<Arc<Session>> {
		Some(self.clone())
	}
}

impl<T: PubSubMetadata> PubSubMetadata for Option<T> {
	fn session(&self) -> Option<Arc<Session>> {
		self.as_ref().and_then(|s| s.session())
	}
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

impl From<String> for SubscriptionId {
	fn from(other: String) -> Self {
		SubscriptionId::String(other)
	}
}

impl From<u64> for SubscriptionId {
	fn from(other: u64) -> Self {
		SubscriptionId::Number(other)
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

#[cfg(test)]
mod tests {
	use core::Value;
	use super::SubscriptionId;

	#[test]
	fn should_convert_between_value_and_subscription_id() {
		// given
		let val1 = Value::Number(5.into());
		let val2 = Value::String("asdf".into());
		let val3 = Value::Null;

		// when
		let res1 = SubscriptionId::parse_value(&val1);
		let res2 = SubscriptionId::parse_value(&val2);
		let res3 = SubscriptionId::parse_value(&val3);

		// then
		assert_eq!(res1, Some(SubscriptionId::Number(5)));
		assert_eq!(res2, Some(SubscriptionId::String("asdf".into())));
		assert_eq!(res3, None);

		// and back
		assert_eq!(Value::from(res1.unwrap()), val1);
		assert_eq!(Value::from(res2.unwrap()), val2);
	}
}
