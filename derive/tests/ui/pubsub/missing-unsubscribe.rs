#[macro_use]
extern crate jsonrpc_derive;
extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;

#[rpc]
pub trait Rpc {
	type Metadata;

	/// Hello subscription
	#[pubsub(subscription = "hello", subscribe, name = "hello_subscribe", alias("hello_sub"))]
	fn subscribe(&self, _: Self::Metadata, _: typed::Subscriber<String>, _: u64);

	// note that the unsubscribe method is missing
}

fn main() {}
