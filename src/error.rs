//! jsonrpc errors
use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use super::Value;

#[derive(Debug, PartialEq, Clone)]
pub enum ErrorCode {
	/// Invalid JSON was received by the server.
	/// An error occurred on the server while parsing the JSON text.
	ParseError,
	/// The JSON sent is not a valid Request object.
	InvalidRequest,
	/// The method does not exist / is not available.
	MethodNotFound,
	/// Invalid method parameter(s).
	InvalidParams,
	/// Internal JSON-RPC error.
	InternalError,
	/// Reserved for implementation-defined server-errors.
	ServerError(i64)
}

impl ErrorCode {
	pub fn code(&self) -> i64 {
		match *self {
			ErrorCode::ParseError => -32700,
			ErrorCode::InvalidRequest => -32600,
			ErrorCode::MethodNotFound => -32601,
			ErrorCode::InvalidParams => -32602,
			ErrorCode::InternalError => -32603,
			ErrorCode::ServerError(code) => code
		}
	}

	pub fn description(&self) -> String {
		let desc = match *self {
			ErrorCode::ParseError => "Parse error",
			ErrorCode::InvalidRequest => "Invalid request",
			ErrorCode::MethodNotFound => "Method not found",
			ErrorCode::InvalidParams => "Invalid params",
			ErrorCode::InternalError => "Internal error",
			ErrorCode::ServerError(_) => "Server error",
		};
		desc.to_string()
	}
}

impl Deserialize for ErrorCode {
	fn deserialize<D>(deserializer: &mut D) -> Result<ErrorCode, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		match v.as_i64() {
			Some(-32700) => Ok(ErrorCode::ParseError),
			Some(-32600) => Ok(ErrorCode::InvalidRequest),
			Some(-32601) => Ok(ErrorCode::MethodNotFound),
			Some(-32602) => Ok(ErrorCode::InvalidParams),
			Some(-32603) => Ok(ErrorCode::InternalError),
			Some(code) => Ok(ErrorCode::ServerError(code)),
			_ => unreachable!()
		}

	}
}

impl Serialize for ErrorCode {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		serializer.serialize_i64(self.code())
	}
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Error {
	pub code: ErrorCode,
	pub message: String,
	pub data: Option<Value>
}

impl Error {
	pub fn new(code: ErrorCode) -> Self {
		Error {
			message: code.description(),
			code: code,
			data: None
		}
	}

	pub fn parse_error() -> Self {
		Self::new(ErrorCode::ParseError)
	}

	pub fn invalid_request() -> Self {
		Self::new(ErrorCode::InvalidRequest)
	}

	pub fn method_not_found() -> Self {
		Self::new(ErrorCode::MethodNotFound)
	}

	pub fn invalid_params() -> Self {
		Self::new(ErrorCode::InvalidParams)
	}

	pub fn internal_error() -> Self {
		Self::new(ErrorCode::InternalError)
	}
}
