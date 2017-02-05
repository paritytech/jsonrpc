//! jsonrpc params field
use std::fmt;

use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde::de::{Visitor, SeqVisitor, MapVisitor};
use serde::de::impls::{VecVisitor};
use serde_json;
use serde_json::value::from_value;

use super::{Value, Error};

/// Request parameters
#[derive(Debug, PartialEq)]
pub enum Params {
	/// Array of values
	Array(Vec<Value>),
	/// Map of values
	Map(serde_json::Map<String, Value>),
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
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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
	fn deserialize<D>(deserializer: D) -> Result<Params, D::Error>
	where D: Deserializer {
		deserializer.deserialize(ParamsVisitor)
	}
}

impl Visitor for ParamsVisitor {
	type Value = Params;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a map or sequence")
	}

	fn visit_seq<V>(self, visitor: V) -> Result<Self::Value, V::Error>
	where V: SeqVisitor {
		VecVisitor::new().visit_seq(visitor).and_then(|vec| match vec.is_empty() {
			true => Ok(Params::None),
			false => Ok(Params::Array(vec))
		})
	}

	fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
	where V: MapVisitor {
		let mut values = serde_json::Map::with_capacity(visitor.size_hint().0);

		while let Some((key, value)) = visitor.visit()? {
			values.insert(key, value);
		}

		Ok(if values.is_empty() { Params::None } else { Params::Map(values) })
	}
}

#[test]
fn params_deserialization() {
	use serde_json;

	let s = r#"[null, true, -1, 4, 2.3, "hello", [0], {"key": "value"}]"#;
	let deserialized: Params = serde_json::from_str(s).unwrap();

	let mut map = serde_json::Map::new();
	map.insert("key".to_string(), Value::String("value".to_string()));

	assert_eq!(Params::Array(vec![
							 Value::Null, Value::Bool(true), Value::from(-1), Value::from(4),
							 Value::from(2.3), Value::String("hello".to_string()),
							 Value::Array(vec![Value::from(0)]), Value::Object(map)]), deserialized);
}
