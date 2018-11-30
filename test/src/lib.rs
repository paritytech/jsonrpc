//! An utility package to test jsonrpc-core based projects.
//!
//! ```
//! #[macro_use]
//! extern crate jsonrpc_macros;
//!
//! extern crate jsonrpc_core as core;
//! extern crate jsonrpc_test as test;
//!
//! use core::Result;
//!
//! build_rpc_trait! {
//!   pub trait Test {
//!     #[rpc(name = "rpc_some_method")]
//!	    fn some_method(&self, u64) -> Result<u64>;
//!   }
//! }
//!
//! struct Dummy;
//! impl Test for Dummy {
//!	  fn some_method(&self, x: u64) -> Result<u64> {
//!     Ok(x * 2)
//!	  }
//! }
//!
//! fn main() {
//!   // Initialize new instance of test environment
//!   let rpc = test::Rpc::new(Dummy.to_delegate());
//!
//!   // make a request and verify the response as a pretty-printed string
//!   assert_eq!(rpc.request("rpc_some_method", &[5]), r#"10"#);
//!
//!   // You can also test RPC created without macros:
//!   let rpc = {
//!     let mut io = core::IoHandler::new();
//!     io.add_method("rpc_test_method", |_| {
//!		  Err(core::Error::internal_error())
//!		});
//!     test::Rpc::from(io)
//!   };
//!
//!   assert_eq!(rpc.request("rpc_test_method", &()), r#"{
//!   "code": -32603,
//!   "message": "Internal error"
//! }"#);
//! }
//! ```

#[warn(missing_docs)]

extern crate jsonrpc_core as rpc;
extern crate serde;
extern crate serde_json;

use std::collections::HashMap;

/// Test RPC options.
#[derive(Default, Debug)]
pub struct Options {
	/// Disable printing requests and responses.
	pub no_print: bool,
}

#[derive(Default, Debug)]
/// RPC instance.
pub struct Rpc {
	/// Underlying `IoHandler`.
	pub io: rpc::IoHandler,
	/// Options
	pub options: Options,
}

impl From<rpc::IoHandler> for Rpc {
	fn from(io: rpc::IoHandler) -> Self {
		Rpc { io, ..Default::default() }
	}
}

impl Rpc {
	/// Create a new RPC instance from a single delegate.
	pub fn new<D>(delegate: D) -> Self where
		D: Into<HashMap<String, rpc::RemoteProcedure<()>>>,
	{
		let mut io = rpc::IoHandler::new();
		io.extend_with(delegate);
		io.into()
	}

	fn request_helper<T>(
		&self,
		method: &str,
		params: &T,
		success_formatter: fn(&serde_json::Value) -> serde_json::Result<String>,
		error_formatter: fn(&rpc::Error) -> serde_json::Result<String>,
	) -> String
	where
		T: serde::Serialize,
	{
		use self::rpc::types::response;

		let request = format!(
			"{{ \"jsonrpc\":\"2.0\", \"id\": 1, \"method\": \"{}\", \"params\": {} }}",
			method,
			serde_json::to_string_pretty(params).expect("Serialization should be infallible."),
		);

		let response = self.io
			.handle_request_sync(&request)
			.expect("We are sending a method call not notification.");

		// extract interesting part from the response
		let extracted = match serde_json::from_str(&response).expect("We will always get a single output.") {
			response::Output::Success(response::Success { result, .. }) => success_formatter(&result),
			response::Output::Failure(response::Failure { error, .. }) => error_formatter(&error),
		}.expect("Serialization is infallible; qed");

		println!("\n{}\n --> {}\n", request, extracted);

		extracted
	}

	/// Perform a single, synchronous method call and return pretty-printed value
	pub fn request<T>(&self, method: &str, params: &T) -> String where
		T: serde::Serialize,
	{
		let success_formatter = serde_json::to_string_pretty;
		let error_formatter = serde_json::to_string_pretty;
		self.request_helper(method, params, success_formatter, error_formatter)
	}

	/// Perform a single, synchronous method call.
	pub fn make_request<T>(&self, method: &str, params: &T) -> String where
		T: serde::Serialize,
	{
		let success_formatter = serde_json::to_string;
		let error_formatter = serde_json::to_string;
		self.request_helper(method, params, success_formatter, error_formatter)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn should_test_simple_method() {
		// given
		let rpc = {
			let mut io = rpc::IoHandler::new();
			io.add_method("test_method", |_| {
				Ok(rpc::Value::Array(vec![5.into(), 10.into()]))
			});
			Rpc::from(io)
		};

		// when
		assert_eq!(
			rpc.request("test_method", &[5u64]),
			"[\n  5,\n  10\n]"
		);
	}

	#[test]
	fn should_test_make_request() {
		// given
		let rpc = {
			let mut io = rpc::IoHandler::new();
			io.add_method("test_method", |_| {
				Ok(rpc::Value::Array(vec![5.into(), 10.into()]))
			});
			Rpc::from(io)
		};

		// when
		assert_eq!(
			rpc.make_request("test_method", &[5u64]),
			"[5,10]"
		);
	}
}
