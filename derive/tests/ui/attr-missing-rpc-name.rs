extern crate serde;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;

use jsonrpc_core::Result;

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc]
	fn protocol_version(&self) -> Result<String>;
}

struct RpcImpl;

impl Rpc for RpcImpl {
	fn protocol_version(&self) -> Result<String> {
		Ok("version1".into())
	}
}

fn main() {}
