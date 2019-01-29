use jsonrpc_core;

use jsonrpc_core::{IoHandler, Error, Result};
use jsonrpc_core::futures::future::{self, FutureResult};
use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc<One, Two> {
	/// Get One type.
	#[rpc(name = "getOne")]
	fn one(&self) -> Result<One>;

	/// Adds two numbers and returns a result
	#[rpc(name = "setTwo")]
	fn set_two(&self, _: Two) -> Result<()>;

	/// Performs asynchronous operation
	#[rpc(name = "beFancy")]
	fn call(&self, _: One) -> FutureResult<(One, Two), Error>;
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

	fn call(&self, num: u64) -> FutureResult<(u64, String), Error> {
		crate::future::finished((num + 999, "hello".into()))
	}
}


fn main() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate())
}
