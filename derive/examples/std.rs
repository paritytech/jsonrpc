extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;

use jsonrpc_core::{IoHandler, Result};

use jsonrpc_core::{IoHandler, Error, Result};
use jsonrpc_core::futures::future::{self, FutureResult};

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> Result<String>;

	/// Adds two numbers and returns a result
	#[rpc(name = "add", alias("callAsyncMetaAlias"))]
	fn add(&self, u64, u64) -> Result<u64>;

	/// Performs asynchronous operation
	#[rpc(name = "callAsync")]
	fn call(&self, u64) -> FutureResult<String, Error>;
}

struct RpcImpl;

impl Rpc for RpcImpl {
	fn protocol_version(&self) -> Result<String> {
		Ok("version1".into())
	}

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn call(&self, _: u64) -> FutureResult<String, Error> {
		future::ok("OK".to_owned())
	}
}

fn main() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate())
}