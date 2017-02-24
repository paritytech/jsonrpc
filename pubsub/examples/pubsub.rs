extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
extern crate jsonrpc_http_server;

use std::{time, thread};

use jsonrpc_core::*;
use jsonrpc_pubsub::*;
use jsonrpc_tcp_server::*;

use jsonrpc_core::futures::Future;

#[derive(Clone, Default)]
struct Meta(usize);
impl Metadata for Meta {}
impl PubSubMetadata for Meta {
	fn session(&self) -> Option<Arc<Session>> {
		None
	}
}

fn main() {
	let mut io = PubSubHandler::new(MetaIoHandler::default());
	io.add_method("say_hello", |_params: Params| {
		Ok(Value::String("hello".to_string()))
	});

	io.add_subscription(
		"hello",
		("subscribe_hello", |params: Params, _, subscriber: Subscriber| {
			if params != Params::None {
				subscriber.reject(Error {
					code: ErrorCode::ParseError,
					message: "Invalid paramters. Subscription rejected.".into(),
					data: None,
				});
				return;
			}

			let sink = subscriber.assign_id(SubscriptionId::Number(5));
			// or subscriber.reject(Error {} );
			// or drop(subscriber)
			thread::spawn(move || {
				loop {
					thread::sleep(time::Duration::from_millis(100));
					sink.send(Params::Array(vec![Value::Number(10.into())]));
				}
			});
		}),
		("remove_hello", |_id: SubscriptionId| -> futures::BoxFuture<Value, Error> {
			futures::future::ok(Value::Bool(true)).boxed()
		}),
	);

	let server = ServerBuilder::new(io)
		.meta_extractor(|_session: &hyper::server::Request<hyper::net::HttpStream>| {
			Meta(5)
		})
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

