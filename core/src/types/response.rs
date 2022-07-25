//! jsonrpc response
#[cfg(feature = "nonstrict")]
use std::collections::HashMap;

use super::{Error, ErrorCode, Id, Value, Version};
use crate::Result as CoreResult;

/// Successful response
#[cfg(not(feature = "nonstrict"))]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Success {
	/// Protocol version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Result
	pub result: Value,
	/// Correlation id
	pub id: Id,
}

/// Successful response
#[cfg(feature = "nonstrict")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Success {
	/// Protocol version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Result
	pub result: Value,
	/// Correlation id
	pub id: Id,
	/// Other fields
	#[serde(flatten)]
	pub other: HashMap<String, Value>,
}

/// Unsuccessful response
#[cfg(not(feature = "nonstrict"))]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
	/// Protocol Version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Error
	pub error: Error,
	/// Correlation id
	pub id: Id,
}

/// Unsuccessful response
#[cfg(feature = "nonstrict")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Failure {
	/// Protocol Version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Error
	pub error: Error,
	/// Correlation id
	pub id: Id,
	/// Other fields
	#[serde(flatten)]
	pub other: HashMap<String, Value>,
}

/// Represents output - failure or success
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum Output {
	/// Failure
	Failure(Failure),
	/// Success
	Success(Success),
}

impl Output {
	/// Creates new Success given `Result`, `Id` and `Version`.
	pub fn success(result: Value, id: Id, jsonrpc: Option<Version>) -> Self {
		Output::Success(Success {
			jsonrpc,
			result,
			id,
			#[cfg(feature = "nonstrict")]
			other: HashMap::new(),
		})
	}

	/// Creates new Failure given `Error`, `Id` and `Version`.
	pub fn failure(error: Error, id: Id, jsonrpc: Option<Version>) -> Self {
		Output::Failure(Failure {
			jsonrpc,
			error,
			id,
			#[cfg(feature = "nonstrict")]
			other: HashMap::new(),
		})
	}

	/// Creates new output given `Result`, `Id` and `Version`.
	pub fn from(result: CoreResult<Value>, id: Id, jsonrpc: Option<Version>) -> Self {
		match result {
			Ok(result) => Output::success(result, id, jsonrpc),
			Err(error) => Output::failure(error, id, jsonrpc),
		}
	}

	/// Creates new failure output indicating malformed request.
	pub fn invalid_request(id: Id, jsonrpc: Option<Version>) -> Self {
		Output::Failure(Failure {
			id,
			jsonrpc,
			error: Error::new(ErrorCode::InvalidRequest),
			#[cfg(feature = "nonstrict")]
			other: HashMap::new(),
		})
	}

	/// Get the jsonrpc protocol version.
	pub fn version(&self) -> Option<Version> {
		match *self {
			Output::Success(ref s) => s.jsonrpc,
			Output::Failure(ref f) => f.jsonrpc,
		}
	}

	/// Get the correlation id.
	pub fn id(&self) -> &Id {
		match *self {
			Output::Success(ref s) => &s.id,
			Output::Failure(ref f) => &f.id,
		}
	}
}

impl From<Output> for CoreResult<Value> {
	/// Convert into a result. Will be `Ok` if it is a `Success` and `Err` if `Failure`.
	fn from(output: Output) -> CoreResult<Value> {
		match output {
			Output::Success(s) => Ok(s.result),
			Output::Failure(f) => Err(f.error),
		}
	}
}

/// Synchronous response
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
			#[cfg(feature = "nonstrict")]
			other: HashMap::new(),
		}
		.into()
	}

	/// Deserialize `Response` from given JSON string.
	///
	/// This method will handle an empty string as empty batch response.
	pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
		if s.is_empty() {
			Ok(Response::Batch(vec![]))
		} else {
			crate::serde_from_str(s)
		}
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
		#[cfg(feature = "nonstrict")]
		other: HashMap::new(),
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
		Output::success(Value::from(1), Id::Num(1), Some(Version::V2))
	);
}

#[test]
fn failure_output_serialize() {
	use serde_json;

	let fo = Output::failure(Error::parse_error(), Id::Num(1), Some(Version::V2));

	let serialized = serde_json::to_string(&fo).unwrap();
	assert_eq!(
		serialized,
		r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#
	);
}

#[test]
fn failure_output_serialize_jsonrpc_1() {
	use serde_json;

	let fo = Output::failure(Error::parse_error(), Id::Num(1), None);

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
		Output::failure(Error::parse_error(), Id::Num(1), Some(Version::V2))
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
		Response::Single(Output::success(Value::from(1), Id::Num(1), Some(Version::V2)))
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
			Output::success(Value::from(1), Id::Num(1), Some(Version::V2)),
			Output::failure(Error::parse_error(), Id::Num(1), Some(Version::V2))
		])
	);
}

#[test]
fn handle_incorrect_responses() {
	use serde_json;

	let dsr = r#"
{
	"id": 2,
	"jsonrpc": "2.0",
	"result": "0x62d3776be72cc7fa62cad6fe8ed873d9bc7ca2ee576e400d987419a3f21079d5",
	"error": {
		"message": "VM Exception while processing transaction: revert",
		"code": -32000,
		"data": {}
	}
}"#;

	let deserialized: Result<Response, _> = serde_json::from_str(dsr);
	#[cfg(not(feature = "nonstrict"))]
	assert!(
		deserialized.is_err(),
		"Expected error when deserializing invalid payload."
	);

	#[cfg(feature = "nonstrict")]
	assert_eq!(deserialized.unwrap(), Response::Single(Output::Failure(Failure {
		error: Error {
			code: ErrorCode::ServerError(-32000),
			message: "VM Exception while processing transaction: revert".to_string(),
			data: Some(serde_json::from_str("{}").unwrap()),
			other: HashMap::new(),
		},
		jsonrpc: Some(Version::V2),
		id: Id::Num(2),
		other: serde_json::from_str("{\"result\": \"0x62d3776be72cc7fa62cad6fe8ed873d9bc7ca2ee576e400d987419a3f21079d5\"}").unwrap(),
	})));
}

#[test]
fn should_parse_empty_response_as_batch() {
	use serde_json;

	let dsr = r#""#;

	let deserialized1: Result<Response, _> = serde_json::from_str(dsr);
	let deserialized2: Result<Response, _> = Response::from_json(dsr);
	assert!(
		deserialized1.is_err(),
		"Empty string is not valid JSON, so we should get an error."
	);
	assert_eq!(deserialized2.unwrap(), Response::Batch(vec![]));
}
