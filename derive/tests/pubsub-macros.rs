use jsonrpc_core;
use jsonrpc_pubsub;
use serde_json;
#[macro_use]
extern crate jsonrpc_derive;

use jsonrpc_core::futures::sync::mpsc;
use jsonrpc_pubsub::typed::Subscriber;
use jsonrpc_pubsub::{PubSubHandler, PubSubMetadata, Session, SubscriptionId};
use std::sync::Arc;

pub enum MyError {}
impl From<MyError> for jsonrpc_core::Error {
	fn from(_e: MyError) -> Self {
		unreachable!()
	}
}

type Result<T> = ::std::result::Result<T, MyError>;

#[rpc]
pub trait Rpc {
	type Metadata;

	/// Hello subscription.
	#[pubsub(subscription = "hello", subscribe, name = "hello_subscribe", alias("hello_alias"))]
	fn subscribe(&self, a: Self::Metadata, b: Subscriber<String>, c: u32, d: Option<u64>);

	/// Unsubscribe from hello subscription.
	#[pubsub(subscription = "hello", unsubscribe, name = "hello_unsubscribe")]
	fn unsubscribe(&self, a: Option<Self::Metadata>, b: SubscriptionId) -> Result<bool>;

	/// A regular rpc method alongside pubsub.
	#[rpc(name = "add")]
	fn add(&self, a: u64, b: u64) -> Result<u64>;

	/// A notification alongside pubsub.
	#[rpc(name = "notify")]
	fn notify(&self, a: u64);
}

#[derive(Default)]
struct RpcImpl;

impl Rpc for RpcImpl {
	type Metadata = Metadata;

	fn subscribe(&self, _meta: Self::Metadata, subscriber: Subscriber<String>, _pre: u32, _trailing: Option<u64>) {
		let _sink = subscriber.assign_id(SubscriptionId::Number(5));
	}

	fn unsubscribe(&self, _meta: Option<Self::Metadata>, _id: SubscriptionId) -> Result<bool> {
		Ok(true)
	}

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn notify(&self, a: u64) {
		println!("Received `notify` with value: {}", a);
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
			"message": "`params` should have at least 1 argument(s)"
		},
		"id": 1
	}"#;

	let expected: jsonrpc_core::Response = serde_json::from_str(expected).unwrap();
	let result: jsonrpc_core::Response = serde_json::from_str(&res.unwrap()).unwrap();
	assert_eq!(expected, result);
}

#[test]
fn test_subscribe_with_alias() {
	let mut io = PubSubHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let meta = Metadata;
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello_alias","params":[1]}"#;
	let res = io.handle_request_sync(req, meta);
	let expected = r#"{
		"jsonrpc": "2.0",
		"result": 5,
		"id": 1
	}"#;

	let expected: jsonrpc_core::Response = serde_json::from_str(expected).unwrap();
	let result: jsonrpc_core::Response = serde_json::from_str(&res.unwrap()).unwrap();
	assert_eq!(expected, result);
}
