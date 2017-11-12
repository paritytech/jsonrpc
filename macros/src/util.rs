//! Param & Value utilities

use std::fmt;
use jsonrpc_core::{self as core, Error, Params, ErrorCode, Value};
use serde;

/// Returns an `InvalidParams` for given parameter.
pub fn invalid_params<T>(param: &str, details: T) -> Error where T: fmt::Debug {
	Error {
		code: ErrorCode::InvalidParams,
		message: format!("Couldn't parse parameters: {}", param),
		data: Some(Value::String(format!("{:?}", details))),
	}
}

/// Validates if the method was invoked without any params.
pub fn expect_no_params(params: Params) -> core::Result<()> {
	match params {
		Params::None => Ok(()),
		p => Err(invalid_params("No parameters were expected", p)),
	}
}

/// Converts a serializable value into `Value`.
pub fn to_value<T>(value: T) -> Value where T: serde::Serialize {
	core::to_value(value).expect("Expected always-serializable type.")
}
