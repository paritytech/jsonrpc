//! jsonrpc response
use serde::{Serialize, Serializer};
use super::{Id, Value, Error, Version};

#[derive(Debug, PartialEq, Serialize)]
pub struct Success {
	pub jsonrpc: Version,
	pub result: Value,
	pub id: Id
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Failure {
	pub jsonrpc: Version,
	pub error: Error,
	pub id: Id
}

#[derive(Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
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
fn failure_output_serialize() {
	use serde_json;
	use serde_json::Value;

	let fo = Output::Failure(Failure {
		jsonrpc: Version::V2,
		error: Error::parse_error(),
		id: Id::Num(1)
	});

	let serialized = serde_json::to_string(&fo).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#);
}

