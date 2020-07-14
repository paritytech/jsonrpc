extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server;
extern crate jsonrpc_pubsub;

use std::sync::Arc;
use std::{thread, time};

use jsonrpc_core::*;
use jsonrpc_ipc_server::{RequestContext, ServerBuilder, SessionId, SessionStats};
use jsonrpc_pubsub::{PubSubHandler, Session, Subscriber, SubscriptionId};

/// To test the server:
///
/// ```bash
/// $ netcat localhost 3030 -
/// {"id":1,"jsonrpc":"2.0","method":"hello_subscribe","params":[10]}
///
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
					thread::sleep(time::Duration::from_millis(100));
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
		("remove_hello", |_id: SubscriptionId, _meta| {
			println!("Closing subscription");
			futures::future::ready(Ok(Value::Bool(true)))
		}),
	);

	let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
		Arc::new(Session::new(context.sender.clone()))
	})
	.session_stats(Stats)
	.start("./test.ipc")
	.expect("Unable to start RPC server");

	server.wait();
}

struct Stats;
impl SessionStats for Stats {
	fn open_session(&self, id: SessionId) {
		println!("Opening new session: {}", id);
	}

	fn close_session(&self, id: SessionId) {
		println!("Closing session: {}", id);
	}
}
