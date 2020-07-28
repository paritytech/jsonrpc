extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
extern crate jsonrpc_ws_server;

use std::sync::Arc;
use std::{thread, time};

use jsonrpc_core::*;
use jsonrpc_pubsub::{PubSubHandler, Session, Subscriber, SubscriptionId};
use jsonrpc_ws_server::{RequestContext, ServerBuilder};

/// Use following node.js code to test:
///
/// ```js
/// const WebSocket = require('websocket').w3cwebsocket;
///
/// const ws = new WebSocket('ws://localhost:3030');
/// ws.addEventListener('open', () => {
///   console.log('Sending request');
///
///   ws.send(JSON.stringify({
///     jsonrpc: "2.0",
///     id: 1,
///     method: "subscribe_hello",
///     params: null,
///   }));
/// });
///
/// ws.addEventListener('message', (message) => {
///   console.log('Received: ', message.data);
/// });
///
/// console.log('Starting');
/// ```
fn main() {
	let mut io = PubSubHandler::new(MetaIoHandler::default());
	io.add_sync_method("say_hello", |_params: Params| Ok(Value::String("hello".to_string())));

	io.add_subscription(
		"hello",
		("subscribe_hello", |params: Params, _, subscriber: Subscriber| {
			if params != Params::None {
				subscriber
					.reject(Error {
						code: ErrorCode::ParseError,
						message: "Invalid parameters. Subscription rejected.".into(),
						data: None,
					})
					.unwrap();
				return;
			}

			thread::spawn(move || {
				let sink = subscriber.assign_id(SubscriptionId::Number(5)).unwrap();
				// or subscriber.reject(Error {} );
				// or drop(subscriber)

				loop {
					thread::sleep(time::Duration::from_millis(1000));
					match sink.notify(Params::Array(vec![Value::Number(10.into())])) {
						Ok(_) => {}
						Err(_) => {
							println!("Subscription has ended, finishing.");
							break;
						}
					}
				}
			});
		}),
		(
			"remove_hello",
			|_id: SubscriptionId, _meta| -> BoxFuture<Result<Value>> {
				println!("Closing subscription");
				Box::pin(futures::future::ready(Ok(Value::Bool(true))))
			},
		),
	);

	let server =
		ServerBuilder::with_meta_extractor(io, |context: &RequestContext| Arc::new(Session::new(context.sender())))
			.start(&"127.0.0.1:3030".parse().unwrap())
			.expect("Unable to start RPC server");

	let _ = server.wait();
}
