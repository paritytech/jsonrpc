//! jsonrpc params field
use std::fmt;

use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde::de::{Visitor, SeqAccess, MapAccess, DeserializeOwned};
use serde_json;
use serde_json::value::from_value;

use super::{Value, Error};

/// Request parameters
#[derive(Debug, PartialEq, Clone)]
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
	pub fn parse<D>(self) -> Result<D, Error> where D: DeserializeOwned {
		let value = match self {
			Params::Array(vec) => Value::Array(vec),
			Params::Map(map) => Value::Object(map),
			Params::None =>  Value::Null
		};

		from_value(value).map_err(|e| {
			Error::invalid_params(format!("Invalid params: {}.", e))
		})
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

impl<'a> Deserialize<'a> for Params {
	fn deserialize<D>(deserializer: D) -> Result<Params, D::Error>
	where D: Deserializer<'a> {
		deserializer.deserialize_any(ParamsVisitor)
	}
}

impl<'a> Visitor<'a> for ParamsVisitor {
	type Value = Params;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a map or sequence")
	}

	fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
	where V: SeqAccess<'a> {
		let mut values = Vec::new();

		while let Some(value) = visitor.next_element()? {
			values.push(value);
		}

		if values.is_empty() {
			Ok(Params::None)
		} else {
			Ok(Params::Array(values))
		}
	}

	fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
	where V: MapAccess<'a> {
		let mut values = serde_json::Map::new();

		while let Some((key, value)) = visitor.next_entry()? {
			values.insert(key, value);
		}

		Ok(if values.is_empty() { Params::None } else { Params::Map(values) })
	}
}

#[cfg(test)]
mod tests {
	use serde_json;
	use super::Params;
	use types::{Value, Error, ErrorCode};

	#[test]
	fn params_deserialization() {

		let s = r#"[null, true, -1, 4, 2.3, "hello", [0], {"key": "value"}]"#;
		let deserialized: Params = serde_json::from_str(s).unwrap();

		let mut map = serde_json::Map::new();
		map.insert("key".to_string(), Value::String("value".to_string()));

		assert_eq!(Params::Array(vec![
								 Value::Null, Value::Bool(true), Value::from(-1), Value::from(4),
								 Value::from(2.3), Value::String("hello".to_string()),
								 Value::Array(vec![Value::from(0)]), Value::Object(map)]), deserialized);
	}

	#[test]
	fn should_return_meaningful_error_when_deserialization_fails() {
		// given
		let s = r#"[1, true]"#;
		let params = || serde_json::from_str::<Params>(s).unwrap();

		// when
		let v1: Result<(Option<u8>, String), Error> = params().parse();
		let v2: Result<(u8, bool, String), Error> = params().parse();
		let err1 = v1.unwrap_err();
		let err2 = v2.unwrap_err();

		// then
		assert_eq!(err1.code, ErrorCode::InvalidParams);
		assert_eq!(err1.message, "Invalid params: invalid type: boolean `true`, expected a string.");
		assert_eq!(err1.data, None);
		assert_eq!(err2.code, ErrorCode::InvalidParams);
		assert_eq!(err2.message, "Invalid params: invalid length 2, expected a tuple of size 3.");
		assert_eq!(err2.data, None);
	}
}
