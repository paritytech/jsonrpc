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

	/// Hello subscription with probably different impl and mismatched Subscriber type
	#[pubsub(subscription = "hello", subscribe, name = "hello_anotherSubscribe")]
	fn another_subscribe(&self, _: Self::Metadata, _: typed::Subscriber<usize>, _: u64);

	/// Unsubscribe from hello subscription.
	#[pubsub(subscription = "hello", unsubscribe, name = "hello_unsubscribe")]
	fn unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;
	// note that the unsubscribe method is missing
}

fn main() {}
