extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;

use jsonrpc_core::{Result, IoHandler};

#[rpc(server)]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	fn lifetimed<'a>(&'a self, param: &'a u64) -> Result<String>;
}

struct RpcImpl;

impl Rpc for RpcImpl {
	fn lifetimed<'a>(&'a self, _param: &'a u64) -> Result<String> {
        Ok("hey".to_string())
    }
}

fn main() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate())
}
