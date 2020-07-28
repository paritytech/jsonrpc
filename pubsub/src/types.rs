use crate::core;
use crate::core::futures::channel::mpsc;
use std::sync::Arc;

use crate::subscription::Session;

/// Raw transport sink for specific client.
pub type TransportSender = mpsc::UnboundedSender<String>;
/// Raw transport error.
pub type TransportError = mpsc::SendError;
/// Subscription send result.
pub type SinkResult = Result<(), mpsc::TrySendError<String>>;

/// Metadata extension for pub-sub method handling.
///
/// NOTE storing `PubSubMetadata` (or rather storing `Arc<Session>`) in
/// any other place outside of the handler will prevent `unsubscribe` methods
/// to be called in case the `Session` is dropped (i.e. transport connection is closed).
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
///
/// NOTE Assigning same id to different requests will cause the previous request to be unsubscribed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionId {
	/// A numerical ID, represented by a `u64`.
	Number(u64),
	/// A non-numerical ID, for example a hash.
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

impl From<SubscriptionId> for core::Value {
	fn from(sub: SubscriptionId) -> Self {
		match sub {
			SubscriptionId::Number(val) => core::Value::Number(val.into()),
			SubscriptionId::String(val) => core::Value::String(val),
		}
	}
}

macro_rules! impl_from_num {
	($num:ty) => {
		impl From<$num> for SubscriptionId {
			fn from(other: $num) -> Self {
				SubscriptionId::Number(other.into())
			}
		}
	};
}

impl_from_num!(u8);
impl_from_num!(u16);
impl_from_num!(u32);
impl_from_num!(u64);

#[cfg(test)]
mod tests {
	use super::SubscriptionId;
	use crate::core::Value;

	#[test]
	fn should_convert_between_number_value_and_subscription_id() {
		let val = Value::Number(5.into());
		let res = SubscriptionId::parse_value(&val);

		assert_eq!(res, Some(SubscriptionId::Number(5)));
		assert_eq!(Value::from(res.unwrap()), val);
	}

	#[test]
	fn should_convert_between_string_value_and_subscription_id() {
		let val = Value::String("asdf".into());
		let res = SubscriptionId::parse_value(&val);

		assert_eq!(res, Some(SubscriptionId::String("asdf".into())));
		assert_eq!(Value::from(res.unwrap()), val);
	}

	#[test]
	fn should_convert_between_null_value_and_subscription_id() {
		let val = Value::Null;
		let res = SubscriptionId::parse_value(&val);
		assert_eq!(res, None);
	}

	#[test]
	fn should_convert_from_u8_to_subscription_id() {
		let val = 5u8;
		let res: SubscriptionId = val.into();
		assert_eq!(res, SubscriptionId::Number(5));
	}

	#[test]
	fn should_convert_from_u16_to_subscription_id() {
		let val = 5u16;
		let res: SubscriptionId = val.into();
		assert_eq!(res, SubscriptionId::Number(5));
	}

	#[test]
	fn should_convert_from_u32_to_subscription_id() {
		let val = 5u32;
		let res: SubscriptionId = val.into();
		assert_eq!(res, SubscriptionId::Number(5));
	}

	#[test]
	fn should_convert_from_u64_to_subscription_id() {
		let val = 5u64;
		let res: SubscriptionId = val.into();
		assert_eq!(res, SubscriptionId::Number(5));
	}

	#[test]
	fn should_convert_from_string_to_subscription_id() {
		let val = "String".to_string();
		let res: SubscriptionId = val.into();
		assert_eq!(res, SubscriptionId::String("String".to_string()));
	}
}
