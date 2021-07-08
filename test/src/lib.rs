//! An utility package to test jsonrpc-core based projects.
//!
//! ```
//! use jsonrpc_derive::rpc;
//! use jsonrpc_test as test;
//!
//! use jsonrpc_core::{Result, Error, IoHandler};
//!
//! #[rpc]
//! pub trait Test {
//!    #[rpc(name = "rpc_some_method")]
//!    fn some_method(&self, a: u64) -> Result<u64>;
//! }
//!
//!
//! struct Dummy;
//! impl Test for Dummy {
//!    fn some_method(&self, x: u64) -> Result<u64> {
//!        Ok(x * 2)
//!    }
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
//!     let mut io = IoHandler::new();
//!     io.add_sync_method("rpc_test_method", |_| {
//!        Err(Error::internal_error())
//!     });
//!     test::Rpc::from(io)
//!   };
//!
//!   assert_eq!(rpc.request("rpc_test_method", &()), r#"{
//!   "code": -32603,
//!   "message": "Internal error"
//! }"#);
//! }
//! ```

#![deny(missing_docs)]

extern crate jsonrpc_core as rpc;

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

/// Encoding format.
pub enum Encoding {
	/// Encodes params using `serde::to_string`.
	Compact,
	/// Encodes params using `serde::to_string_pretty`.
	Pretty,
}

impl From<rpc::IoHandler> for Rpc {
	fn from(io: rpc::IoHandler) -> Self {
		Rpc {
			io,
			..Default::default()
		}
	}
}

impl Rpc {
	/// Create a new RPC instance from a single delegate.
	pub fn new<D>(delegate: D) -> Self
	where
		D: IntoIterator<Item = (String, rpc::RemoteProcedure<()>)>,
	{
		let mut io = rpc::IoHandler::new();
		io.extend_with(delegate);
		io.into()
	}

	/// Perform a single, synchronous method call and return pretty-printed value
	pub fn request<T>(&self, method: &str, params: &T) -> String
	where
		T: serde::Serialize,
	{
		self.make_request(method, params, Encoding::Pretty)
	}

	/// Perform a single, synchronous method call.
	pub fn make_request<T>(&self, method: &str, params: &T, encoding: Encoding) -> String
	where
		T: serde::Serialize,
	{
		use self::rpc::types::response;

		let request = format!(
			"{{ \"jsonrpc\":\"2.0\", \"id\": 1, \"method\": \"{}\", \"params\": {} }}",
			method,
			serde_json::to_string_pretty(params).expect("Serialization should be infallible."),
		);

		let response = self
			.io
			.handle_request_sync(&request)
			.expect("We are sending a method call not notification.");

		// extract interesting part from the response
		let extracted = match rpc::serde_from_str(&response).expect("We will always get a single output.") {
			response::Output::Success(response::Success { result, .. }) => match encoding {
				Encoding::Compact => serde_json::to_string(&result),
				Encoding::Pretty => serde_json::to_string_pretty(&result),
			},
			response::Output::Failure(response::Failure { error, .. }) => match encoding {
				Encoding::Compact => serde_json::to_string(&error),
				Encoding::Pretty => serde_json::to_string_pretty(&error),
			},
		}
		.expect("Serialization is infallible; qed");

		println!("\n{}\n --> {}\n", request, extracted);

		extracted
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn should_test_request_is_pretty() {
		// given
		let rpc = {
			let mut io = rpc::IoHandler::new();
			io.add_sync_method("test_method", |_| Ok(rpc::Value::Array(vec![5.into(), 10.into()])));
			Rpc::from(io)
		};

		// when
		assert_eq!(rpc.request("test_method", &[5u64]), "[\n  5,\n  10\n]");
	}

	#[test]
	fn should_test_make_request_compact() {
		// given
		let rpc = {
			let mut io = rpc::IoHandler::new();
			io.add_sync_method("test_method", |_| Ok(rpc::Value::Array(vec![5.into(), 10.into()])));
			Rpc::from(io)
		};

		// when
		assert_eq!(rpc.make_request("test_method", &[5u64], Encoding::Compact), "[5,10]");
	}
}
