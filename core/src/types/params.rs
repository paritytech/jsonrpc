//! jsonrpc params field
use std::collections::BTreeMap;

use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde::de::{Visitor, SeqVisitor, MapVisitor};
use serde::de::impls::{VecVisitor, BTreeMapVisitor};
use serde_json::value::from_value;

use super::{Value, Error};

/// Request parameters
#[derive(Debug, PartialEq)]
pub enum Params {
	/// Array of values
	Array(Vec<Value>),
	/// Map of values
	Map(BTreeMap<String, Value>),
	/// No parameters
	None
}

impl Params {
	/// Parse incoming `Params` into expected types.
	pub fn parse<D>(self) -> Result<D, Error> where D: Deserialize {
		let value = match self {
			Params::Array(vec) => Value::Array(vec),
			Params::Map(map) => Value::Object(map),
			Params::None =>  Value::Null
		};

		from_value(value).map_err(|_| Error::invalid_params())
	}
}

impl Serialize for Params {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match *self {
			Params::Array(ref vec) => vec.serialize(serializer),
			Params::Map(ref map) => map.serialize(serializer),
			Params::None => ([0u8; 0]).serialize(serializer)
		}
	}
}

struct ParamsVisitor;

impl Deserialize for Params {
	fn deserialize<D>(deserializer: &mut D) -> Result<Params, D::Error>
	where D: Deserializer {
		deserializer.deserialize(ParamsVisitor)
	}
}

impl Visitor for ParamsVisitor {
	type Value = Params;

	fn visit_seq<V>(&mut self, visitor: V) -> Result<Self::Value, V::Error>
	where V: SeqVisitor {
		VecVisitor::new().visit_seq(visitor).and_then(|vec| match vec.is_empty() {
			true => Ok(Params::None),
			false => Ok(Params::Array(vec))
		})
	}

	fn visit_map<V>(&mut self, visitor: V) -> Result<Self::Value, V::Error>
	where V: MapVisitor {
		BTreeMapVisitor::new().visit_map(visitor).and_then(|map| match map.is_empty() {
			true => Ok(Params::None),
			false => Ok(Params::Map(map))
		})
	}
}

#[test]
fn params_deserialization() {
	use serde_json;

	use std::collections::BTreeMap;

	let s = r#"[null, true, -1, 4, 2.3, "hello", [0], {"key": "value"}]"#;
	let deserialized: Params = serde_json::from_str(s).unwrap();

	let mut map = BTreeMap::new();
	map.insert("key".to_string(), Value::String("value".to_string()));

	assert_eq!(Params::Array(vec![
							 Value::Null, Value::Bool(true), Value::I64(-1), Value::U64(4),
							 Value::F64(2.3), Value::String("hello".to_string()),
							 Value::Array(vec![Value::U64(0)]), Value::Object(map)]), deserialized);
}
