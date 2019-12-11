use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc {
	/// Has too many params
	#[rpc(name = "tooManyParams")]
	fn to_many_params(
        &self,
        a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64, i: u64, j: u64,
        k: u64, l: u64, m: u64, n: u64, o: u64, p: u64, q: u64,
    ) -> Result<String>;
}

fn main() {}
