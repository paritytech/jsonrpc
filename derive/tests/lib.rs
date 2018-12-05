extern crate jsonrpc_core;
#[macro_use] extern crate jsonrpc_derive;

//use jsonrpc_core::{IoHandler, Error, Result};

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> jsonrpc_core::Result<String>;

	/// Adds two numbers and returns a result
	#[rpc(name = "add")]
	fn add(&self, u64, u64) -> jsonrpc_core::Result<u64>;
}

#[test]
fn basic_rpc() {

}
