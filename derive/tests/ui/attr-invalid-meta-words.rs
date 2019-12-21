use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion", Xmeta)]
	fn protocol_version(&self) -> Result<String>;
}

fn main() {}
