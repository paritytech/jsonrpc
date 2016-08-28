//! jsonrpc response
use serde::de::{Deserialize, Deserializer, Error as DeError};
use serde::ser::{Serialize, Serializer};
use serde_json::value::from_value;
use super::{Id, Value, Error, Version, AsyncOutput};

/// Successful response
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Success {
	/// Protocol version
	pub jsonrpc: Version,
	/// Result
	pub result: Value,
	/// Correlation id
	pub id: Id
}

/// Unsuccessful response
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
	/// Protocol Version
	pub jsonrpc: Version,
	/// Error
	pub error: Error,
	/// Correlation id
	pub id: Id
}

/// Represents synchronous output - failure or success
#[derive(Debug, PartialEq)]
pub enum SyncOutput {
	/// Success
	Success(Success),
	/// Failure
	Failure(Failure),
}

/// Represents synchronous or asynchronous output
#[derive(Debug)]
pub enum Output {
	/// Synchronous
	Sync(SyncOutput),
	/// Asynchronous
	Async(AsyncOutput),
}

impl SyncOutput {
	/// Creates new synchronous output given `Result`, `Id` and `Version`.
	pub fn from(result: Result<Value, Error>, id: Id, jsonrpc: Version) -> Self {
		match result {
			Ok(result) => SyncOutput::Success(Success {
				id: id,
				jsonrpc: jsonrpc,
				result: result,
			}),
			Err(error) => SyncOutput::Failure(Failure {
				id: id,
				jsonrpc: jsonrpc,
				error: error,
			}),
		}
	}
}

impl Deserialize for SyncOutput {
	fn deserialize<D>(deserializer: &mut D) -> Result<SyncOutput, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		from_value(v.clone()).map(SyncOutput::Failure)
			.or_else(|_| from_value(v).map(SyncOutput::Success))
			.map_err(|_| D::Error::custom("")) // types must match
	}
}

impl Serialize for SyncOutput {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match *self {
			SyncOutput::Success(ref s) => s.serialize(serializer),
			SyncOutput::Failure(ref f) => f.serialize(serializer)
		}
	}
}

/// Synchronous response
#[derive(Debug, PartialEq)]
pub enum SyncResponse {
	/// Single response
	Single(SyncOutput),
	/// Response to batch request (batch of responses)
	Batch(Vec<SyncOutput>)
}

impl Deserialize for SyncResponse {
	fn deserialize<D>(deserializer: &mut D) -> Result<SyncResponse, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		from_value(v.clone()).map(SyncResponse::Batch)
			.or_else(|_| from_value(v).map(SyncResponse::Single))
			.map_err(|_| D::Error::custom("")) // types must match
	}
}

impl Serialize for SyncResponse {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match *self {
			SyncResponse::Single(ref o) => o.serialize(serializer),
			SyncResponse::Batch(ref b) => b.serialize(serializer)
		}
	}
}

impl From<Failure> for SyncResponse {
	fn from(failure: Failure) -> Self {
		SyncResponse::Single(SyncOutput::Failure(failure))
	}
}

impl From<Success> for SyncResponse {
	fn from(success: Success) -> Self {
		SyncResponse::Single(SyncOutput::Success(success))
	}
}

#[test]
fn success_output_serialize() {
	use serde_json;
	use serde_json::Value;

	let so = SyncOutput::Success(Success {
		jsonrpc: Version::V2,
		result: Value::U64(1),
		id: Id::Num(1)
	});

	let serialized = serde_json::to_string(&so).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","result":1,"id":1}"#);
}

#[test]
fn success_output_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dso = r#"{"jsonrpc":"2.0","result":1,"id":1}"#;

	let deserialized: SyncOutput = serde_json::from_str(dso).unwrap();
	assert_eq!(deserialized, SyncOutput::Success(Success {
		jsonrpc: Version::V2,
		result: Value::U64(1),
		id: Id::Num(1)
	}));
}

#[test]
fn failure_output_serialize() {
	use serde_json;

	let fo = SyncOutput::Failure(Failure {
		jsonrpc: Version::V2,
		error: Error::parse_error(),
		id: Id::Num(1)
	});

	let serialized = serde_json::to_string(&fo).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error","data":null},"id":1}"#);
}

#[test]
fn failure_output_deserialize() {
	use serde_json;

	let dfo = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#;

	let deserialized: SyncOutput = serde_json::from_str(dfo).unwrap();
	assert_eq!(deserialized, SyncOutput::Failure(Failure {
		jsonrpc: Version::V2,
		error: Error::parse_error(),
		id: Id::Num(1)
	}));
}

#[test]
fn single_response_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dsr = r#"{"jsonrpc":"2.0","result":1,"id":1}"#;

	let deserialized: SyncResponse = serde_json::from_str(dsr).unwrap();
	assert_eq!(deserialized, SyncResponse::Single(SyncOutput::Success(Success {
		jsonrpc: Version::V2,
		result: Value::U64(1),
		id: Id::Num(1)
	})));
}

#[test]
fn batch_response_deserialize() {
	use serde_json;
	use serde_json::Value;

	let dbr = r#"[{"jsonrpc":"2.0","result":1,"id":1},{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}]"#;

	let deserialized: SyncResponse = serde_json::from_str(dbr).unwrap();
	assert_eq!(deserialized, SyncResponse::Batch(vec![
		SyncOutput::Success(Success {
			jsonrpc: Version::V2,
			result: Value::U64(1),
			id: Id::Num(1)
		}),
		SyncOutput::Failure(Failure {
			jsonrpc: Version::V2,
			error: Error::parse_error(),
			id: Id::Num(1)
		})
	]));
}
