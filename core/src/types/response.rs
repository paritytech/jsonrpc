//! jsonrpc response
use super::{Error, ErrorCode, Id, Value, Version};
use crate::Result as CoreResult;
use serde::Serialize;

/// Successful response
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Success<T = Value> {
	/// Protocol version
	#[serde(skip_serializing_if = "Option::is_none")]
	pub jsonrpc: Option<Version>,
	/// Result
	pub result: T,
	/// Correlation id
	pub id: Id,
}

/// Unsuccessful response
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

/// Formatted JSON RPC response
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum Output<T = Value> {
	/// Success
	Success(Success<T>),
	/// Failure
	Failure(Failure),
}

impl<T> Output<T> {
	/// Get the jsonrpc protocol version.
	pub fn version(&self) -> Option<Version> {
		match *self {
			Self::Success(ref s) => s.jsonrpc,
			Self::Failure(ref f) => f.jsonrpc,
		}
	}

	/// Get the correlation id.
	pub fn id(&self) -> &Id {
		match *self {
			Self::Success(ref s) => &s.id,
			Self::Failure(ref f) => &f.id,
		}
	}
}

impl From<Output<Value>> for CoreResult<Value> {
	/// Convert into a result. Will be `Ok` if it is a `Success` and `Err` if `Failure`.
	fn from(output: Output<Value>) -> CoreResult<Value> {
		match output {
			Output::Success(s) => Ok(s.result),
			Output::Failure(f) => Err(f.error),
		}
	}
}

/// Synchronous response
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
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

/// Represents output, failure or success
///
/// This contains the full response string, including the jsonrpc envelope.
#[derive(Debug, PartialEq, Clone)]
pub struct SerializedOutput {
	/// Response jsonrpc json
	pub response: String,
}

impl SerializedOutput {
	/// Creates new output given `Result`, `Id` and `Version`.
	pub fn from<T>(result: CoreResult<T>, id: Id, jsonrpc: Option<Version>) -> Self
	where
		T: Serialize,
	{
		match result {
			Ok(result) => {
				let response = serde_json::to_string(&Success { jsonrpc, result, id })
					.expect("Expected always-serializable type; qed");
				SerializedOutput { response }
			}
			Err(error) => Self::from_error(error, id, jsonrpc),
		}
	}

	/// Create new output from an `Error`, `Id` and `Version`.
	pub fn from_error(error: Error, id: Id, jsonrpc: Option<Version>) -> Self {
		let response =
			serde_json::to_string(&Failure { jsonrpc, error, id }).expect("Expected always-serializable type; qed");
		SerializedOutput { response }
	}

	/// Creates new failure output indicating malformed request.
	pub fn invalid_request(id: Id, jsonrpc: Option<Version>) -> Self {
		Self::from_error(Error::new(ErrorCode::InvalidRequest), id, jsonrpc)
	}
}

/// Synchronous response, pre-serialized
#[derive(Clone, Debug, PartialEq)]
pub enum SerializedResponse {
	/// Single response
	Single(SerializedOutput),
	/// Response to batch request (batch of responses)
	Batch(Vec<SerializedOutput>),
}

impl SerializedResponse {
	/// Creates new `Response` with given error and `Version`
	pub fn from(error: Error, jsonrpc: Option<Version>) -> Self {
		Self::Single(SerializedOutput::from_error(error, Id::Null, jsonrpc))
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

	let fo: Output = Output::Failure(Failure {
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

	let fo: Output = Output::Failure(Failure {
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
	assert!(
		deserialized.is_err(),
		"Expected error when deserializing invalid payload."
	);
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
