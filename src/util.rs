use serde::{Deserialize, Serialize};
use serde_json::value::{Value, Deserializer, Serializer};
use super::{Params, Error};

pub fn from_params<D>(params: Params) -> Result<D, Error> where D: Deserialize {
	let value = match params {
		Params::Array(vec) => Value::Array(vec),
		Params::Map(map) => Value::Object(map),
		Params::None =>  Value::Null
	};
	
	let mut deserializer = Deserializer::new(value);
	Deserialize::deserialize(&mut deserializer).map_err(|_| Error::invalid_params())
}

pub fn to_value<S>(s: &S) -> Result<Value, Error> where S: Serialize {
	let mut serializer = Serializer::new();
	match s.serialize(&mut serializer) {
		Err(_) => Err(Error::internal_error()),
		_ => Ok(serializer.unwrap())
	}
}

