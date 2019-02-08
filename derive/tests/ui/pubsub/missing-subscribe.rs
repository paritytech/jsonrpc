#[macro_use]
extern crate jsonrpc_derive;
extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;

#[rpc]
pub trait Rpc {
	type Metadata;

	// note that a subscribe method is missing

	/// Unsubscribe from hello subscription.
	#[pubsub(subscription = "hello", unsubscribe, name = "hello_unsubscribe")]
	fn unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;
}

fn main() {}
