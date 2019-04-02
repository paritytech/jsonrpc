extern crate serde;
extern crate jsonrpc_core;
extern crate jsonrpc_client;
#[macro_use]
extern crate jsonrpc_derive;
extern crate log;

use jsonrpc_core::futures::future::Future;
use jsonrpc_core::futures::sync::mpsc;

#[rpc(client)]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> Result<String>;

	/// Adds two numbers and returns a result
	#[rpc(name = "add", alias("callAsyncMetaAlias"))]
	fn add(&self, a: u64, b: u64) -> Result<u64>;
}

fn main() {
	let fut = {
		let (sender, _) = mpsc::channel(0);
		gen_client::Client::new(sender)
			.add(5, 6)
			.map(|res| println!("5 + 6 = {}", res))
	};
	fut
		.wait()
		.ok();
}
