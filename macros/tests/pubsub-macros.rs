extern crate serde_json;
extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
#[macro_use]
extern crate jsonrpc_macros;

use std::sync::Arc;
use jsonrpc_core::futures::sync::mpsc;
use jsonrpc_pubsub::{PubSubHandler, SubscriptionId, Session, PubSubMetadata};
use jsonrpc_macros::{pubsub, Trailing};

pub enum MyError {}
impl From<MyError> for jsonrpc_core::Error {
	fn from(_e: MyError) -> Self {
		unreachable!()
	}
}

type Result<T> = ::std::result::Result<T, MyError>;

build_rpc_trait! {
	pub trait Rpc {
		type Metadata;

		#[pubsub(name = "hello")] {
			/// Hello subscription
			#[rpc(name = "hello_subscribe")]
			fn subscribe(&self, Self::Metadata, pubsub::Subscriber<String>, u32, Trailing<u64>);

			/// Unsubscribe from hello subscription.
			#[rpc(name = "hello_unsubscribe")]
			fn unsubscribe(&self, SubscriptionId) -> Result<bool>;
		}
	}
}

#[derive(Default)]
struct RpcImpl;

impl Rpc for RpcImpl {
	type Metadata = Metadata;

	fn subscribe(&self, _meta: Self::Metadata, subscriber: pubsub::Subscriber<String>, _pre: u32, _trailing: Trailing<u64>) {
		let _sink = subscriber.assign_id(SubscriptionId::Number(5));
	}

	fn unsubscribe(&self, _id: SubscriptionId) -> Result<bool> {
		Ok(true)
	}
}

#[derive(Clone, Default)]
struct Metadata;
impl jsonrpc_core::Metadata for Metadata {}
impl PubSubMetadata for Metadata {
	fn session(&self) -> Option<Arc<Session>> {
		let (tx, _rx) = mpsc::channel(1);
		Some(Arc::new(Session::new(tx)))
	}
}

#[test]
fn test_invalid_trailing_pubsub_params() {
	let mut io = PubSubHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let meta = Metadata;
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello_subscribe","params":[]}"#;
	let res = io.handle_request_sync(req, meta);
	let expected = r#"{
		"jsonrpc": "2.0",
		"error": {
			"code": -32602,
			"message": "Couldn't parse parameters: `params` should have at least 1 argument(s)",
			"data": "\"\""
		},
		"id": 1
	}"#;

	let expected: jsonrpc_core::Response = serde_json::from_str(expected).unwrap();
	let result: jsonrpc_core::Response = serde_json::from_str(&res.unwrap()).unwrap();
	assert_eq!(expected, result);
}
