extern crate serde;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_macros;

use serde::de::DeserializeOwned;
use jsonrpc_core::{IoHandler, Error, Result};
use jsonrpc_core::futures::future::{self, FutureResult};

// Two only requires DeserializeOwned
build_rpc_trait! {
	pub trait Rpc<One> where
		Two: DeserializeOwned,
	{
		/// Get One type.
		#[rpc(name = "getOne")]
		fn one(&self) -> Result<One>;

		/// Adds two numbers and returns a result
		#[rpc(name = "setTwo")]
		fn set_two(&self, Two) -> Result<()>;

		/// Performs asynchronous operation
		#[rpc(name = "beFancy")]
		fn call(&self, One) -> FutureResult<(One, u64), Error>;
	}
}

build_rpc_trait! {
	pub trait Rpc2<> where
		Two: DeserializeOwned,
	{
		/// Adds two numbers and returns a result
		#[rpc(name = "setTwo")]
		fn set_two(&self, Two) -> Result<()>;
	}
}

struct RpcImpl;

impl Rpc<u64, String> for RpcImpl {
	fn one(&self) -> Result<u64> {
		Ok(100)
	}

	fn set_two(&self, x: String) -> Result<()> {
		println!("{}", x);
		Ok(())
	}

	fn call(&self, num: u64) -> FutureResult<(u64, u64), Error> {
		::future::finished((num + 999, num))
	}
}

impl Rpc2<String> for RpcImpl {
	fn set_two(&self, _: String) -> Result<()> {
		unimplemented!()
	}
}


fn main() {
	let mut io = IoHandler::new();

	io.extend_with(Rpc::to_delegate(RpcImpl));
	io.extend_with(Rpc2::to_delegate(RpcImpl));
}

