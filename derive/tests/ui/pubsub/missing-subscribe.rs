#[macro_use]
extern crate jsonrpc_derive;
extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;

#[rpc(pubsub, name = "hello")]
pub trait Rpc {
	type Metadata;

	/// Hello subscription
	#[rpc(name = "hello_subscribe", alias("hello_sub"))]
	fn subscribe(&self, Self::Metadata, typed::Subscriber<String>, u64);

	/// Unsubscribe from hello subscription.
	#[rpc(unsubscribe, name = "hello_unsubscribe")]
	fn unsubscribe(&self, Option<Self::Metadata>, SubscriptionId) -> Result<bool>;
}

fn main() {}
