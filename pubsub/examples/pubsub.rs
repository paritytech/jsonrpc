extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
extern crate jsonrpc_http_server;

use std::sync::Arc;
use std::{time, thread};

use jsonrpc_core::*;
use jsonrpc_pubsub::*;
use jsonrpc_http_server::*;

use jsonrpc_core::futures::Future;

#[derive(Clone, Default)]
struct Meta(usize);
impl Metadata for Meta {}
impl PubSubMetadata for Meta {
	fn send(data: String) {
		unimplemented!()
	}
}

fn main() {
	let mut io = PubSubHandler::default();
	io.add_method("say_hello", |_params: Params| {
		Ok(Value::String("hello".to_string()))
	});

	io.add_subscription(
		"hello",
		("subscribe_hello", |params: Params, _, subscriber: Subscriber<Meta>| {
			if params != Params::None {
				subscriber.reject(Error {
					code: ErrorCode::ParseError,
					message: "Invalid paramters. Subscription rejected.".into(),
					data: None,
				});
				return;
			}

			let sink = subscriber.assign_id(Value::Number(5.into()));
			// or subscriber.reject(Error {} );
			// or drop(subscriber)
			thread::spawn(move || {
				loop {
					thread::sleep(time::Duration::from_millis(100));
					sink.send(Value::Number(10.into()));
				}
			});
		}),
		("remove_hello", |id: SubscriptionId| -> futures::BoxFuture<bool, Error> {
			futures::future::ok(true).boxed()
		}),
	);

	let server = ServerBuilder::new(io)
		.meta_extractor(Arc::new(|_session: &hyper::server::Request<hyper::net::HttpStream>| {
			Meta(5)
		}))
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

