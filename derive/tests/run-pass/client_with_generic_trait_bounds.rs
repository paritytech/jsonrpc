use jsonrpc_core::futures::future;
use jsonrpc_core::{IoHandler, Result, BoxFuture};
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
	fn call(&self, a: One) -> BoxFuture<Result<(One, Two)>>;
}

struct RpcImpl;

impl Rpc<u64, String> for RpcImpl {
	fn set_two(&self, x: String) -> Result<BTreeMap<u64, ()>> {
		println!("{}", x);
		Ok(Default::default())
	}

	fn call(&self, num: u64) -> BoxFuture<Result<(u64, String)>> {
		Box::pin(future::ready(Ok((num + 999, "hello".into()))))
	}
}

fn main() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate())
}
