//! jsonrpc request
use super::{Id, Params, Version};

/// Represents jsonrpc request which is a method call.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MethodCall<M = String, P = Params> {
	/// A String specifying the version of the JSON-RPC protocol.
	pub jsonrpc: Option<Version>,
	/// A String containing the name of the method to be invoked.
	pub method: M,
	/// A Structured value that holds the parameter values to be used
	/// during the invocation of the method. This member MAY be omitted.
	pub params: Option<P>,
	/// An identifier established by the Client that MUST contain a String,
	/// Number, or NULL value if included. If it is not included it is assumed
	/// to be a notification.
	pub id: Id,
}

/// Represents jsonrpc request which is a notification.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notification<M = String, P = Params> {
	/// A String specifying the version of the JSON-RPC protocol.
	pub jsonrpc: Option<Version>,
	/// A String containing the name of the method to be invoked.
	pub method: M,
	/// A Structured value that holds the parameter values to be used
	/// during the invocation of the method. This member MAY be omitted.
	pub params: Option<P>,
}

/// Represents single jsonrpc call.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Call<M = String, P = Params> {
	/// Call method
	MethodCall(MethodCall<M, P>),
	/// Fire notification
	Notification(Notification<M, P>),
	/// Invalid call with an ID
	Invalid {
		/// Request id
		#[serde(default = "default_id")]
		id: Id
	},
}

fn default_id() -> Id {
	Id::Null
}

impl<M, P> From<MethodCall<M, P>> for Call<M, P> {
	fn from(mc: MethodCall<M, P>) -> Self {
		Call::MethodCall(mc)
	}
}

impl<M, P> From<Notification<M, P>> for Call<M, P> {
	fn from(n: Notification<M, P>) -> Self {
		Call::Notification(n)
	}
}

/// Represents jsonrpc request.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Request<M = String, P = Params> {
	/// Single request (call)
	Single(Call<M, P>),
	/// Batch of requests (calls)
	Batch(Vec<Call<M, P>>)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json;
	use serde_json::Value;

	#[test]
	fn method_call_serialize() {
		let m = MethodCall {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1), Value::from(2)])),
			id: Id::Num(1)
		};

		let serialized = serde_json::to_string(&m).unwrap();
		assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1,2],"id":1}"#);
	}

	#[test]
	fn notification_serialize() {
		let n = Notification {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1), Value::from(2)]))
		};

		let serialized = serde_json::to_string(&n).unwrap();
		assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1,2]}"#);
	}

	#[test]
	fn call_serialize() {
		let n = Call::Notification(Notification {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1)]))
		});

		let serialized = serde_json::to_string(&n).unwrap();
		assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1]}"#);
	}

	#[test]
	fn request_serialize_batch() {
		let batch = Request::Batch(vec![
			Call::MethodCall(MethodCall {
				jsonrpc: Some(Version::V2),
				method: "update".to_owned(),
				params: Some(Params::Array(vec![Value::from(1), Value::from(2)])),
				id: Id::Num(1)
			}),
			Call::Notification(Notification {
				jsonrpc: Some(Version::V2),
				method: "update".to_owned(),
				params: Some(Params::Array(vec![Value::from(1)]))
			})
		]);

		let serialized = serde_json::to_string(&batch).unwrap();
		assert_eq!(serialized, r#"[{"jsonrpc":"2.0","method":"update","params":[1,2],"id":1},{"jsonrpc":"2.0","method":"update","params":[1]}]"#);

	}

	#[test]
	fn notification_deserialize() {
		let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1,2]}"#;
		let deserialized: Notification = serde_json::from_str(s).unwrap();

		assert_eq!(deserialized, Notification {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1), Value::from(2)]))
		});

		let s = r#"{"jsonrpc": "2.0", "method": "foobar"}"#;
		let deserialized: Notification = serde_json::from_str(s).unwrap();

		assert_eq!(deserialized, Notification {
			jsonrpc: Some(Version::V2),
			method: "foobar".to_owned(),
			params: None
		});

		let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1,2], "id": 1}"#;
		let deserialized: Result<Notification, _> = serde_json::from_str(s);
		assert!(deserialized.is_err())
	}

	#[test]
	fn call_deserialize() {
		let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1]}"#;
		let deserialized: Call = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, Call::Notification(Notification {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1)]))
		}));

		let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1], "id": 1}"#;
		let deserialized: Call = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::from(1)])),
			id: Id::Num(1)
		}));
	}

	#[test]
	fn request_deserialize_batch() {
		let s = r#"[{}, {"jsonrpc": "2.0", "method": "update", "params": [1,2], "id": 1},{"jsonrpc": "2.0", "method": "update", "params": [1]}]"#;
		let deserialized: Request = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, Request::Batch(vec![
			Call::Invalid { id: Id::Null },
			Call::MethodCall(MethodCall {
				jsonrpc: Some(Version::V2),
				method: "update".to_owned(),
				params: Some(Params::Array(vec![Value::from(1), Value::from(2)])),
				id: Id::Num(1)
			}),
			Call::Notification(Notification {
				jsonrpc: Some(Version::V2),
				method: "update".to_owned(),
				params: Some(Params::Array(vec![Value::from(1)]))
			})
		]))
	}

	#[test]
	fn request_invalid_returns_id() {
		let s = r#"{"id":120,"method":"my_method","params":["foo", "bar"],"extra_field":[]}"#;
		let deserialized: Request = serde_json::from_str(s).unwrap();
		match deserialized {
			Request::Single(Call::Invalid { id: Id::Num(120) }) => {},
			_ => panic!("Request wrongly deserialized: {:?}", deserialized),
		}
	}
}
