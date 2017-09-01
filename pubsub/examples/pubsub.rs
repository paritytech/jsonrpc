extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
extern crate jsonrpc_tcp_server;

use std::{time, thread};
use std::sync::Arc;

use jsonrpc_core::*;
use jsonrpc_pubsub::{PubSubHandler, PubSubMetadata, Session, Subscriber, SubscriptionId};
use jsonrpc_tcp_server::{ServerBuilder, RequestContext};

use jsonrpc_core::futures::Future;

#[derive(Clone)]
struct Meta {
	session: Option<Arc<Session>>,
}

impl Default for Meta {
	fn default() -> Self {
		Meta {
			session: None,
		}
	}
}

impl Metadata for Meta {}
impl PubSubMetadata for Meta {
	fn session(&self) -> Option<Arc<Session>> {
		self.session.clone()
	}
}

/// To test the server:
///
/// ```bash
/// $ netcat localhost 3030 -
/// {"id":1,"jsonrpc":"2.0","method":"hello_subscribe","params":[10]}
///
/// ```
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
					message: "Invalid parameters. Subscription rejected.".into(),
					data: None,
				}).unwrap();
				return;
			}

			let sink = subscriber.assign_id(SubscriptionId::Number(5)).unwrap();
			// or subscriber.reject(Error {} );
			// or drop(subscriber)
			thread::spawn(move || {
				loop {
					thread::sleep(time::Duration::from_millis(100));
					match sink.notify(Params::Array(vec![Value::Number(10.into())])).wait() {
						Ok(_) => {},
						Err(_) => {
							println!("Subscription has ended, finishing.");
							break;
						}
					}
				}
			});
		}),
		("remove_hello", |_id: SubscriptionId| -> BoxFuture<Value, Error> {
			println!("Closing subscription");
			Box::new(futures::future::ok(Value::Bool(true)))
		}),
	);

	let server = ServerBuilder::new(io)
		.session_meta_extractor(|context: &RequestContext| {
			Meta {
				session: Some(Arc::new(Session::new(context.sender.clone()))),
			}
		})
		.start(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait();
}

