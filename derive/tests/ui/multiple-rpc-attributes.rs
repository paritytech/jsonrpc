extern crate serde;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	#[rpc(name = "protocolVersionAgain")]
	fn protocol_version(&self) -> Result<String>;
}

fn main() {}
