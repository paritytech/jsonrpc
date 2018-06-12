extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
#[macro_use]
extern crate jsonrpc_macros;
extern crate jsonrpc_tcp_server;

use std::thread;
use std::sync::{atomic, Arc, RwLock};
use std::collections::HashMap;

use jsonrpc_core::{Error, ErrorCode, Result};
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::{Session, PubSubHandler, SubscriptionId};

use jsonrpc_macros::pubsub;

build_rpc_trait! {
	pub trait Rpc {
		type Metadata;

		/// Adds two numbers and returns a result
		#[rpc(name = "add")]
		fn add(&self, u64, u64) -> Result<u64>;

		#[pubsub(name = "hello")] {
			/// Hello subscription
			#[rpc(name = "hello_subscribe", alias = ["hello_sub", ])]
			fn subscribe(&self, Self::Metadata, pubsub::Subscriber<String>, u64);

			/// Unsubscribe from hello subscription.
			#[rpc(name = "hello_unsubscribe")]
			fn unsubscribe(&self, SubscriptionId) -> Result<bool>;
		}
	}
}

#[derive(Default)]
struct RpcImpl {
	uid: atomic::AtomicUsize,
	active: Arc<RwLock<HashMap<SubscriptionId, pubsub::Sink<String>>>>,
}
impl Rpc for RpcImpl {
	type Metadata = Arc<Session>;

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn subscribe(&self, _meta: Self::Metadata, subscriber: pubsub::Subscriber<String>, param: u64) {
		if param != 10 {
			subscriber.reject(Error {
				code: ErrorCode::InvalidParams,
				message: "Rejecting subscription - invalid parameters provided.".into(),
				data: None,
			}).unwrap();
			return;
		}

		let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
		let sub_id = SubscriptionId::Number(id as u64);
		let sink = subscriber.assign_id(sub_id.clone()).unwrap();
		self.active.write().unwrap().insert(sub_id, sink);
	}

	fn unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
		let removed = self.active.write().unwrap().remove(&id);
		if removed.is_some() {
			Ok(true)
		} else {
			Err(Error {
				code: ErrorCode::InvalidParams,
				message: "Invalid subscription.".into(),
				data: None,
			})
		}
	}
}


fn main() {
	let mut io = PubSubHandler::default();
	let rpc = RpcImpl::default();
	let active_subscriptions = rpc.active.clone();

	thread::spawn(move || {
		loop {
			{
				let subscribers = active_subscriptions.read().unwrap();
				for sink in subscribers.values() {
					let _ = sink.notify(Ok("Hello World!".into())).wait();
				}
			}
			thread::sleep(::std::time::Duration::from_secs(1));
		}
	});

	io.extend_with(rpc.to_delegate());

	let server = jsonrpc_tcp_server::ServerBuilder
		::with_meta_extractor(io, |context: &jsonrpc_tcp_server::RequestContext| Arc::new(Session::new(context.sender.clone())))
		.start(&"0.0.0.0:3030".parse().unwrap())
		.expect("Server must start with no issues");

	server.wait()
}
