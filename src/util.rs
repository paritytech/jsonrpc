//! Serialization / Deserialization utilities.
use serde::{Deserialize, Serialize};
use serde_json::value::{Value, from_value, self};
use super::{Params, Error};

/// Parse incoming `Params` into expected types.
pub fn from_params<D>(params: Params) -> Result<D, Error> where D: Deserialize {
	let value = match params {
		Params::Array(vec) => Value::Array(vec),
		Params::Map(map) => Value::Object(map),
		Params::None =>  Value::Null
	};

	from_value(value).map_err(|_| Error::invalid_params())
}

/// Converts serializable output into `Value`
pub fn to_value<S>(s: &S) -> Result<Value, Error> where S: Serialize {
	Ok(value::to_value(s))
}

