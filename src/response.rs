//! jsonrpc response
use serde::{Serialize, Serializer};
use super::{Id, Value, Error, Version};

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

#[derive(Debug, PartialEq, Deserialize)]
pub enum Output {
	Success(Success),
	Failure(Failure)
}

impl Serialize for Output {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error> 
	where S: Serializer {
		match *self {
			Output::Success(ref s) => s.serialize(serializer),
			Output::Failure(ref f) => f.serialize(serializer)
		}
	}
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum Response {
	Single(Output),
	Batch(Vec<Output>)
}

impl Serialize for Response {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error> 
	where S: Serializer {
		match *self {
			Response::Single(ref o) => o.serialize(serializer),
			Response::Batch(ref b) => b.serialize(serializer)
		}
	}
}

#[test]
fn success_output_serialize() {
	use serde_json;
	use serde_json::Value;

	let so = Output::Success(Success {
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

	let deserialized: Output = serde_json::from_str(dso).unwrap();
	assert_eq!(deserialized, Output::Success(Success {
		jsonrpc: Version::V2,
		result: Value::U64(1),
		id: Id::Num(1)
	}));
}

#[test]
fn failure_output_serialize() {
	use serde_json;
	use serde_json::Value;

	let fo = Output::Failure(Failure {
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
	use serde_json::Value;

	let dfo = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#;

	let deserialized: Output = serde_json::from_str(dfo).unwrap();
	assert_eq!(deserialized, Output::Failure(Failure {
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

	let deserialized: Response = serde_json::from_str(dsr).unwrap();
	assert_eq!(deserialized, Response::Single(Output::Success(Success {
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

	let deserialized: Response = serde_json::from_str(dbr).unwrap();
	assert_eq!(deserialized, Response::Batch(vec![
		Output::Success(Success {
			jsonrpc: Version::V2,
			result: Value::U64(1),
			id: Id::Num(1)
		}),
		Output::Failure(Failure {
			jsonrpc: Version::V2,
			error: Error::parse_error(),
			id: Id::Num(1)
		})
	]));
}



