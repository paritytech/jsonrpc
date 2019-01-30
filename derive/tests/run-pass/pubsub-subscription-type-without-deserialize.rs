#[macro_use]
extern crate serde_derive;

use jsonrpc_core::Result;
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::{typed::Subscriber, SubscriptionId};

// One way serialization
#[derive(Serialize)]
struct SerializeOnly {
	foo: String,
}

#[rpc]
pub trait Rpc {
	type Metadata;

	/// Hello subscription
	#[pubsub(subscription = "hello", subscribe, name = "hello_subscribe", alias("hello_sub"))]
	fn subscribe(&self, _: Self::Metadata, _: typed::Subscriber<SerializeOnly>, _: u64);

	/// Unsubscribe from hello subscription.
	#[pubsub(subscription = "hello", unsubscribe, name = "hello_unsubscribe")]
	fn unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;
}

fn main() {
	let _ = Rpc::to_delegate();
}
