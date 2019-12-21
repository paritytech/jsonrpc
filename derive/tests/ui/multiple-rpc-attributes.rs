use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	#[rpc(name = "protocolVersionAgain")]
	fn protocol_version(&self) -> Result<String>;
}

fn main() {}
