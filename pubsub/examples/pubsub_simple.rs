use std::sync::Arc;
use std::{thread, time};

use jsonrpc_core::*;
use jsonrpc_pubsub::{PubSubHandler, Session, Subscriber, SubscriptionId};
use jsonrpc_tcp_server::{RequestContext, ServerBuilder};

use jsonrpc_core::futures::Future;

/// To test the server:
///
/// ```bash
/// $ netcat localhost 3030
/// > {"id":1,"jsonrpc":"2.0","method":"subscribe_hello","params":null}
/// < {"id":1,"jsonrpc":"2.0","result":5,"id":1}
/// < {"jsonrpc":"2.0","method":"hello","params":[10]}
///
/// ```
fn main() {
	let mut io = PubSubHandler::new(MetaIoHandler::default());
	io.add_method("say_hello", |_params: Params| Ok(Value::String("hello".to_string())));

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
				let sink = subscriber.assign_id_async(SubscriptionId::Number(5)).wait().unwrap();
				// or subscriber.reject(Error {} );
				// or drop(subscriber)

				loop {
					thread::sleep(time::Duration::from_millis(100));
					match sink.notify(Params::Array(vec![Value::Number(10.into())])).wait() {
						Ok(_) => {}
						Err(_) => {
							println!("Subscription has ended, finishing.");
							break;
						}
					}
				}
			});
		}),
		("remove_hello", |_id: SubscriptionId, _| {
			println!("Closing subscription");
			futures::future::ok(Value::Bool(true))
		}),
	);

	let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
		Arc::new(Session::new(context.sender.clone()))
	})
	.start(&"127.0.0.1:3030".parse().unwrap())
	.expect("Unable to start RPC server");

	server.wait();
}
