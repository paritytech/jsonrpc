use futures::prelude::*;
use jsonrpc_core::{IoHandler, Result};
use jsonrpc_core_client::transports::local;
use jsonrpc_derive::rpc;

mod client_server {
	use super::*;

	#[rpc(params = "positional")]
	pub trait Rpc {
		#[rpc(name = "add")]
		fn add(&self, a: u64, b: u64) -> Result<u64>;

		#[rpc(name = "notify")]
		fn notify(&self, foo: u64);
	}

	struct RpcServer;

	impl Rpc for RpcServer {
		fn add(&self, a: u64, b: u64) -> Result<u64> {
			Ok(a + b)
		}

		fn notify(&self, foo: u64) {
			println!("received {}", foo);
		}
	}

	#[test]
	fn client_server_roundtrip() {
		let mut handler = IoHandler::new();
		handler.extend_with(RpcServer.to_delegate());
		let (client, rpc_client) = local::connect::<gen_client::Client, _, _>(handler);
		let fut = client
			.clone()
			.add(3, 4)
			.and_then(move |res| client.notify(res).map(move |_| res))
			.join(rpc_client)
			.map(|(res, ())| {
				assert_eq!(res, 7);
			})
			.map_err(|err| {
				eprintln!("{:?}", err);
				assert!(false);
			});
		tokio::run(fut);
	}
}

mod named_params {
	use super::*;
	use jsonrpc_core::Params;
	use serde_json::json;

	#[rpc(client, params = "named")]
	pub trait Rpc {
		#[rpc(name = "call_with_named")]
		fn call_with_named(&self, number: u64, string: String, json: Value) -> Result<Value>;

		#[rpc(name = "notify", params = "raw")]
		fn notify(&self, payload: Value);
	}

	#[test]
	fn client_generates_correct_named_params_payload() {
		let expected = json!({ // key names are derived from function parameter names in the trait
			"number": 3,
			"string": String::from("test string"),
			"json": {
				"key": ["value"]
			}
		});

		let mut handler = IoHandler::new();
		handler.add_method("call_with_named", |params: Params| Ok(params.into()));

		let (client, rpc_client) = local::connect::<gen_client::Client, _, _>(handler);
		let fut = client
			.clone()
			.call_with_named(3, String::from("test string"), json!({"key": ["value"]}))
			.and_then(move |res| client.notify(res.clone()).map(move |_| res))
			.join(rpc_client)
			.map(move |(res, ())| {
				assert_eq!(res, expected);
			})
			.map_err(|err| {
				eprintln!("{:?}", err);
				assert!(false);
			});
		tokio::run(fut);
	}
}

mod raw_params {
	use super::*;
	use jsonrpc_core::Params;
	use serde_json::json;

	#[rpc(client)]
	pub trait Rpc {
		#[rpc(name = "call_raw", params = "raw")]
		fn call_raw_single_param(&self, params: Value) -> Result<Value>;

		#[rpc(name = "notify", params = "raw")]
		fn notify(&self, payload: Value);
	}

	#[test]
	fn client_generates_correct_raw_params_payload() {
		let expected = json!({
			"sub_object": {
				"key": ["value"]
			}
		});

		let mut handler = IoHandler::new();
		handler.add_method("call_raw", |params: Params| Ok(params.into()));

		let (client, rpc_client) = local::connect::<gen_client::Client, _, _>(handler);
		let fut = client
			.clone()
			.call_raw_single_param(expected.clone())
			.and_then(move |res| client.notify(res.clone()).map(move |_| res))
			.join(rpc_client)
			.map(move |(res, ())| {
				assert_eq!(res, expected);
			})
			.map_err(|err| {
				eprintln!("{:?}", err);
				assert!(false);
			});
		tokio::run(fut);
	}
}
