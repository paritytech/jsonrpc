//! jsonrpc id field
use std::fmt;

use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde::de::{self, Visitor};

/// Request Id
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub enum Id {
	/// No id (notification)
	Null,
	/// String id
	Str(String),
	/// Numeric id
	Num(u64),
}

impl Serialize for Id {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where S: Serializer {
		match *self {
			Id::Null => serializer.serialize_unit(),
			Id::Str(ref v) => serializer.serialize_str(v),
			Id::Num(v) => serializer.serialize_u64(v)
		}
	}
}

impl<'a> Deserialize<'a> for Id {
	fn deserialize<D>(deserializer: D) -> Result<Id, D::Error>
	where D: Deserializer<'a> {
		deserializer.deserialize_any(IdVisitor)
	}
}

struct IdVisitor;

impl<'a> Visitor<'a> for IdVisitor {
	type Value = Id;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a unit, integer or string")
	}

	fn visit_unit<E>(self) -> Result<Self::Value, E> where E: de::Error {
		Ok(Id::Null)
	}

	fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> where E: de::Error {
		Ok(Id::Num(value))
	}

	fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> where E: de::Error {
		self.visit_string(value.to_owned())
	}

	fn visit_string<E>(self, value: String) -> Result<Self::Value, E> where E: de::Error {
		value.parse::<u64>().map(Id::Num).or(Ok(Id::Str(value)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json;

	#[test]
	fn id_deserialization() {
		let s = r#""2""#;
		let deserialized: Id = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, Id::Num(2));

		let s = r#""2x""#;
		let deserialized: Id = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, Id::Str("2x".to_owned()));

		let s = r#"[null, 0, 2, "3"]"#;
		let deserialized: Vec<Id> = serde_json::from_str(s).unwrap();
		assert_eq!(deserialized, vec![Id::Null, Id::Num(0), Id::Num(2), Id::Num(3)]);
	}

	#[test]
	fn id_serialization() {
		let d = vec![Id::Null, Id::Num(0), Id::Num(2), Id::Num(3), Id::Str("3".to_owned()), Id::Str("test".to_owned())];
		let serialized = serde_json::to_string(&d).unwrap();
		assert_eq!(serialized, r#"[null,0,2,3,"3","test"]"#);
	}
}
