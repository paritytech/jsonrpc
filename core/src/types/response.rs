//! jsonrpc response
use super::{Error, ErrorCode, Id, Notification, Params, Value, Version};
use crate::Result as CoreResult;

/// Successful response
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Success {
	/// Protocol version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Result
	pub result: Value,
	/// Correlation id
	pub id: Id,
}

/// Unsuccessful response
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Failure {
	/// Protocol Version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Error
	pub error: Error,
	/// Correlation id
	pub id: Id,
}

/// Represents output - failure or success
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Output {
	/// Notification
	Notification(Notification),
	/// Success
	Success(Success),
	/// Failure
	Failure(Failure),
}

impl Output {
	/// Creates new output given `Result`, `Id` and `Version`.
	pub fn from(result: CoreResult<Value>, id: Id, jsonrpc: Option<Version>) -> Self {
		match result {
			Ok(result) => Output::Success(Success { id, jsonrpc, result }),
			Err(error) => Output::Failure(Failure { id, jsonrpc, error }),
		}
	}

	/// Creates new failure output indicating malformed request.
	pub fn invalid_request(id: Id, jsonrpc: Option<Version>) -> Self {
		Output::Failure(Failure {
			id,
			jsonrpc,
			error: Error::new(ErrorCode::InvalidRequest),
		})
	}

	/// Get the jsonrpc protocol version.
	pub fn version(&self) -> Option<Version> {
		match *self {
			Output::Success(ref s) => s.jsonrpc,
			Output::Failure(ref f) => f.jsonrpc,
			Output::Notification(ref n) => n.jsonrpc,
		}
	}

	/// Get the correlation id.
	pub fn id(&self) -> &Id {
		match *self {
			Output::Success(ref s) => &s.id,
			Output::Failure(ref f) => &f.id,
			Output::Notification(_) => &Id::Null,
		}
	}

	/// Get the method name if the output is a notification.
	pub fn method(&self) -> Option<String> {
		match *self {
			Output::Notification(ref n) => Some(n.method.to_owned()),
			_ => None,
		}
	}
}

impl From<Output> for CoreResult<Value> {
	/// Convert into a result. Will be `Ok` if it is a `Success` and `Err` if `Failure`.
	fn from(output: Output) -> CoreResult<Value> {
		match output {
			Output::Success(s) => Ok(s.result),
			Output::Failure(f) => Err(f.error),
			Output::Notification(n) => match &n.params {
				Params::Map(map) => {
					let subscription = map.contains_key("subscription");
					let result = map.contains_key("result");
					let error = map.contains_key("error");
					if subscription && result {
						Ok(map.get("result").unwrap().to_owned())
					} else if subscription && error {
						let err = map.get("error").unwrap().to_owned();
						let error = serde_json::from_value::<Error>(err).expect("should be a jsonrpc error");
						Err(error)
					} else {
						Ok(n.params.into())
					}
				}
				_ => Ok(n.params.into()),
			},
		}
	}
}

/// Synchronous response
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Response {
	/// Single response
	Single(Output),
	/// Response to batch request (batch of responses)
	Batch(Vec<Output>),
}

impl Response {
	/// Creates new `Response` with given error and `Version`
	pub fn from(error: Error, jsonrpc: Option<Version>) -> Self {
		Failure {
			id: Id::Null,
			jsonrpc,
			error,
		}
		.into()
	}
}

impl From<Failure> for Response {
	fn from(failure: Failure) -> Self {
		Response::Single(Output::Failure(failure))
	}
}

impl From<Success> for Response {
	fn from(success: Success) -> Self {
		Response::Single(Output::Success(success))
	}
}

#[test]
fn success_output_serialize() {
	use serde_json;
	use serde_json::Value;

	let so = Output::Success(Success {
		jsonrpc: Some(Version::V2),
		result: Value::from(1),
		id: Id::Num(1),
	});

	let serialized = serde_json::to_string(&so).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","result":1,"id":1}"#);
}

#[test]
fn success_output_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dso = r#"{"jsonrpc":"2.0","result":1,"id":1}"#;

	let deserialized: Output = serde_json::from_str(dso).unwrap();
	assert_eq!(
		deserialized,
		Output::Success(Success {
			jsonrpc: Some(Version::V2),
			result: Value::from(1),
			id: Id::Num(1)
		})
	);
}

#[test]
fn failure_output_serialize() {
	use serde_json;

	let fo = Output::Failure(Failure {
		jsonrpc: Some(Version::V2),
		error: Error::parse_error(),
		id: Id::Num(1),
	});

	let serialized = serde_json::to_string(&fo).unwrap();
	assert_eq!(
		serialized,
		r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#
	);
}

#[test]
fn failure_output_serialize_jsonrpc_1() {
	use serde_json;

	let fo = Output::Failure(Failure {
		jsonrpc: None,
		error: Error::parse_error(),
		id: Id::Num(1),
	});

	let serialized = serde_json::to_string(&fo).unwrap();
	assert_eq!(
		serialized,
		r#"{"error":{"code":-32700,"message":"Parse error"},"id":1}"#
	);
}

#[test]
fn failure_output_deserialize() {
	use serde_json;

	let dfo = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#;

	let deserialized: Output = serde_json::from_str(dfo).unwrap();
	assert_eq!(
		deserialized,
		Output::Failure(Failure {
			jsonrpc: Some(Version::V2),
			error: Error::parse_error(),
			id: Id::Num(1)
		})
	);
}

#[test]
fn single_response_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dsr = r#"{"jsonrpc":"2.0","result":1,"id":1}"#;

	let deserialized: Response = serde_json::from_str(dsr).unwrap();
	assert_eq!(
		deserialized,
		Response::Single(Output::Success(Success {
			jsonrpc: Some(Version::V2),
			result: Value::from(1),
			id: Id::Num(1)
		}))
	);
}

#[test]
fn batch_response_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dbr = r#"[{"jsonrpc":"2.0","result":1,"id":1},{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}]"#;

	let deserialized: Response = serde_json::from_str(dbr).unwrap();
	assert_eq!(
		deserialized,
		Response::Batch(vec![
			Output::Success(Success {
				jsonrpc: Some(Version::V2),
				result: Value::from(1),
				id: Id::Num(1)
			}),
			Output::Failure(Failure {
				jsonrpc: Some(Version::V2),
				error: Error::parse_error(),
				id: Id::Num(1)
			})
		])
	);
}

#[test]
fn notification_deserialize() {
	use super::Params;
	use serde_json;
	use serde_json::Value;

	let dsr = r#"{"jsonrpc":"2.0","method":"hello","params":[10]}"#;
	let deserialized: Response = serde_json::from_str(dsr).unwrap();
	assert_eq!(
		deserialized,
		Response::Single(Output::Notification(Notification {
			jsonrpc: Some(Version::V2),
			method: "hello".into(),
			params: Params::Array(vec![Value::from(10)]),
		}))
	);
}
