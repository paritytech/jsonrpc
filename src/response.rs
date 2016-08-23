//! jsonrpc response
use serde::de::{Deserialize, Deserializer, Error as DeError};
use serde::ser::{Serialize, Serializer};
use serde_json::value;
use super::{Id, Value, Error, Version, AsyncOutput};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Success {
	pub jsonrpc: Version,
	pub result: Value,
	pub id: Id
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
	pub jsonrpc: Version,
	pub error: Error,
	pub id: Id
}

#[derive(Debug, PartialEq)]
pub enum SyncOutput {
	Success(Success),
	Failure(Failure),
}

#[derive(Debug)]
pub enum Output {
	Sync(SyncOutput),
	Async(AsyncOutput),
}

impl SyncOutput {
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
		Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(SyncOutput::Failure)
			.or_else(|_| Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(SyncOutput::Success))
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

#[derive(Debug, PartialEq)]
pub enum SyncResponse {
	Single(SyncOutput),
	Batch(Vec<SyncOutput>)
}

impl Deserialize for SyncResponse {
	fn deserialize<D>(deserializer: &mut D) -> Result<SyncResponse, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(SyncResponse::Batch)
			.or_else(|_| Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(SyncResponse::Single))
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
