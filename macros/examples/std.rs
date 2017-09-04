extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_macros;

use jsonrpc_core::{IoHandler, Error};
use jsonrpc_core::futures::future::{self, FutureResult};

build_rpc_trait! {
	pub trait Rpc {
		/// Returns a protocol version
		#[rpc(name = "protocolVersion")]
		fn protocol_version(&self) -> Result<String, Error>;

		/// Adds two numbers and returns a result
		#[rpc(name = "add")]
		fn add(&self, u64, u64) -> Result<u64, Error>;

		/// Performs asynchronous operation
		#[rpc(name = "callAsync")]
		fn call(&self, u64) -> FutureResult<String, Error>;
	}
}

struct RpcImpl;

impl Rpc for RpcImpl {
	fn protocol_version(&self) -> Result<String, Error> {
		Ok("version1".into())
	}

	fn add(&self, a: u64, b: u64) -> Result<u64, Error> {
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
