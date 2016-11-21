//! jsonrpc errors
use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use super::Value;

/// JSONRPC error code
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
	/// Pub-Sub not supported
	SessionNotSupported,
	/// Reserved for implementation-defined server-errors.
	ServerError(i64)
}

impl ErrorCode {
	/// Returns integer code value
	pub fn code(&self) -> i64 {
		match *self {
			ErrorCode::ParseError => -32700,
			ErrorCode::InvalidRequest => -32600,
			ErrorCode::MethodNotFound => -32601,
			ErrorCode::InvalidParams => -32602,
			ErrorCode::InternalError => -32603,
			ErrorCode::SessionNotSupported => -32001,
			ErrorCode::ServerError(code) => code
		}
	}

	/// Returns human-readable description
	pub fn description(&self) -> String {
		let desc = match *self {
			ErrorCode::ParseError => "Parse error",
			ErrorCode::InvalidRequest => "Invalid request",
			ErrorCode::MethodNotFound => "Method not found",
			ErrorCode::InvalidParams => "Invalid params",
			ErrorCode::InternalError => "Internal error",
			ErrorCode::SessionNotSupported => "Subscriptions are not supported",
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
			Some(-32000) => Ok(ErrorCode::SessionNotSupported),
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

/// Error object as defined in Spec
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Error {
	/// Code
	pub code: ErrorCode,
	/// Message
	pub message: String,
	/// Optional data
	pub data: Option<Value>
}

impl Error {
	/// Wraps given `ErrorCode`
	pub fn new(code: ErrorCode) -> Self {
		Error {
			message: code.description(),
			code: code,
			data: None
		}
	}

	/// Creates new `ParseError`
	pub fn parse_error() -> Self {
		Self::new(ErrorCode::ParseError)
	}

	/// Creates new `InvalidRequest`
	pub fn invalid_request() -> Self {
		Self::new(ErrorCode::InvalidRequest)
	}

	/// Creates new `MethodNotFound`
	pub fn method_not_found() -> Self {
		Self::new(ErrorCode::MethodNotFound)
	}

	/// Creates new `InvalidParams`
	pub fn invalid_params() -> Self {
		Self::new(ErrorCode::InvalidParams)
	}

	/// Creates new `InternalError`
	pub fn internal_error() -> Self {
		Self::new(ErrorCode::InternalError)
	}
}
