use std::fmt;
use jsonrpc_core::{Error, Params, ErrorCode, Value};

pub fn invalid_params<T>(param: &str, details: T) -> Error where T: fmt::Debug {
	Error {
		code: ErrorCode::InvalidParams,
		message: format!("Couldn't parse parameters: {}", param),
		data: Some(Value::String(format!("{:?}", details))),
	}
}

pub fn expect_no_params(params: Params) -> Result<(), Error> {
	match params {
		Params::None => Ok(()),
		p => Err(invalid_params("No parameters were expected", p)),
	}
}

