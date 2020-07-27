use jsonrpc_core::futures::future::{self, FutureResult};
use jsonrpc_core::{Error, IoHandler, Result};
use jsonrpc_derive::rpc;
use std::collections::BTreeMap;

#[rpc]
pub trait Rpc<One, Two>
where
	One: Ord,
	Two: Ord + Eq,
{
	/// Adds two numbers and returns a result
	#[rpc(name = "setTwo")]
	fn set_two(&self, a: Two) -> Result<BTreeMap<One, ()>>;

	/// Performs asynchronous operation
	#[rpc(name = "beFancy")]
	fn call(&self, a: One) -> FutureResult<(One, Two), Error>;
}

struct RpcImpl;

impl Rpc<u64, String> for RpcImpl {
	fn set_two(&self, x: String) -> Result<BTreeMap<u64, ()>> {
		println!("{}", x);
		Ok(Default::default())
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
