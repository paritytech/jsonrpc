extern crate serde;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;

use jsonrpc_core::Result;

#[rpc]
pub trait Rpc {
	/// Has too many params
	#[rpc(name = "tooManyParams")]
	fn to_many_params(
        &self,
        _: u64, _: u64, _: u64, _: u64, _: u64, _: u64, _: u64, _: u64, _: u64, _: u64,
        _: u64, _: u64, _: u64, _: u64, _: u64, _: u64, _: u64,
    ) -> Result<String>;
}

fn main() {}