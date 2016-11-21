//! jsonrpc version field
use serde::{Serialize, Serializer, Deserialize, Deserializer, Error};
use serde::de::Visitor;

/// Protocol Version
#[derive(Debug, PartialEq, Clone)]
pub enum Version {
	/// JSONRPC 2.0
	V2
}

impl Serialize for Version {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match self {
			&Version::V2 => serializer.serialize_str("2.0")
		}
	}
}

impl Deserialize for Version {
	fn deserialize<D>(deserializer: &mut D) -> Result<Version, D::Error>
	where D: Deserializer {
		deserializer.deserialize(VersionVisitor)
	}
}

struct VersionVisitor;

impl Visitor for VersionVisitor {
	type Value = Version;

	fn visit_str<E>(&mut self, value: &str) -> Result<Self::Value, E> where E: Error {
		match value {
			"2.0" => Ok(Version::V2),
			_ => Err(Error::custom("invalid version"))
		}
	}

	fn visit_string<E>(&mut self, value: String) -> Result<Self::Value, E> where E: Error {
		self.visit_str(value.as_ref())
	}
}

