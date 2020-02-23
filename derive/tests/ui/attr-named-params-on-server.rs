use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "add", params = "named")]
	fn add(&self, a: u32, b: u32) -> Result<String>;
}

fn main() {}
