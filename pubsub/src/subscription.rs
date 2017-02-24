use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use core;
use core::futures::{self, sink, future, Sink as FuturesSink, Future, BoxFuture};
use core::futures::sync::oneshot;

use handler::{SubscribeRpcMethod, UnsubscribeRpcMethod};
use types::{PubSubMetadata, SubscriptionId, TransportSender};

/// RPC client session
/// Keeps track of active subscriptions and unsubscribes from them upon dropping.
pub struct Session {
	active_subscriptions: Mutex<HashMap<(SubscriptionId, String), Box<Fn(SubscriptionId) + Send + 'static>>>,
	transport: Mutex<TransportSender>,
}

impl Session {
	/// Creates new session given transport raw send capabilities.
	/// Session should be created as part of metadata, `sender` should be returned by transport.
	pub fn new(sender: TransportSender) -> Self {
		Session {
			active_subscriptions: Default::default(),
			transport: Mutex::new(sender),
		}
	}

	/// Returns transport write stream
	pub fn sender(&self) -> TransportSender {
		self.transport.lock().clone()
	}

	/// Adds new active subscription
	fn add_subscription(&self, name: &str, id: &SubscriptionId, remove: Box<Fn(SubscriptionId) + Send + 'static>) {
		let ret = self.active_subscriptions.lock().insert((id.clone(), name.into()), remove);
		if let Some(remove) = ret {
			warn!("SubscriptionId collision. Unsubscribing previous client.");
			remove(id.clone());
		}
	}

	/// Removes existing subscription.
	fn remove_subscription(&self, name: &str, id: &SubscriptionId) {
		self.active_subscriptions.lock().remove(&(id.clone(), name.into()));
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		let mut active = self.active_subscriptions.lock();
		for (id, remove) in active.drain() {
			remove(id.0)
		}
	}
}

/// A handle to send notifications directly to subscribed client.
pub struct Sink {
	notification: String,
	transport: TransportSender
}

impl Sink {
	/// Sends a notification to a client.
	pub fn send(&self, val: core::Params) -> sink::Send<TransportSender> {
		let notification = core::Notification {
			jsonrpc: Some(core::Version::V2),
			method: self.notification.clone(),
			params: Some(val),
		};

		// TODO [ToDr] Unwraps
		self.transport.clone().send(core::to_string(&notification).unwrap())
	}
}

/// Represents a subscribing client.
/// Subscription handlers can either reject this subscription request or assign an unique id.
pub struct Subscriber {
	notification: String,
	transport: TransportSender,
	sender: oneshot::Sender<Result<SubscriptionId, core::Error>>,
}

impl Subscriber {
	/// Consumes `Subscriber` and assigns unique id to a requestor.
	pub fn assign_id(self, id: SubscriptionId) -> Sink {
		self.sender.complete(Ok(id));

		Sink {
			notification: self.notification,
			transport: self.transport,
		}
	}

	/// Rejects this subscription request with given error.
	pub fn reject(self, error: core::Error) {
		self.sender.complete(Err(error))
	}
}


/// Creates new subscribe and unsubscribe RPC methods
pub fn new_subscription<M, F, G>(notification: &str, subscribe: F, unsubscribe: G) -> (Subscribe<F, G>, Unsubscribe<G>) where
	M: PubSubMetadata,
	F: SubscribeRpcMethod<M>,
	G: UnsubscribeRpcMethod,
{
	let unsubscribe = Arc::new(unsubscribe);
	let subscribe = Subscribe {
		notification: notification.to_owned(),
		subscribe: subscribe,
		unsubscribe: unsubscribe.clone(),
	};

	let unsubscribe = Unsubscribe {
		notification: notification.into(),
		unsubscribe: unsubscribe,
	};

	(subscribe, unsubscribe)
}

fn subscription_rejected() -> core::Error {
	core::Error {
		code: core::ErrorCode::ServerError(-32091),
		message: "Subscription rejected".into(),
		data: None,
	}
}

fn subscriptions_unavailable() -> core::Error {
	core::Error {
		code: core::ErrorCode::ServerError(-32090),
		message: "Subscriptions are not available on this transport.".into(),
		data: None,
	}
}

/// Subscribe RPC implementation.
pub struct Subscribe<F, G> {
	notification: String,
	subscribe: F,
	unsubscribe: Arc<G>,
}

impl<M, F, G> core::RpcMethod<M> for Subscribe<F, G> where
	M: PubSubMetadata,
	F: SubscribeRpcMethod<M>,
	G: UnsubscribeRpcMethod,
{
	fn call(&self, params: core::Params, meta: M) -> BoxFuture<core::Value, core::Error> {
		match meta.session() {
			Some(session) => {
				let (tx, rx) = oneshot::channel();
				let subscriber = Subscriber {
					notification: self.notification.clone(),
					transport: session.sender(),
					sender: tx,
				};

				let unsub = self.unsubscribe.clone();
				let notification = self.notification.clone();
				// Register the subscription
				self.subscribe.call(params, meta, subscriber);

				rx
					.map_err(|_| subscription_rejected())
					.and_then(move |result| {
						futures::done(match result {
							Ok(id) => {
								session.add_subscription(&notification, &id, Box::new(move |id| {
									let _ = unsub.call(id).wait();
								}));
								Ok(id.into())
							},
							Err(e) => Err(e),
						})
					})
					.boxed()
			},
			None => future::err(subscriptions_unavailable()).boxed(),
		}
	}
}

/// Unsubscribe RPC implementation.
pub struct Unsubscribe<G> {
	notification: String,
	unsubscribe: Arc<G>,
}

impl<M, G> core::RpcMethod<M> for Unsubscribe<G> where
	M: PubSubMetadata,
	G: UnsubscribeRpcMethod,
{
	fn call(&self, params: core::Params, meta: M) -> BoxFuture<core::Value, core::Error> {
		let id = match params {
			core::Params::Array(ref vec) if vec.len() == 1 => {
				SubscriptionId::parse_value(&vec[0])
			},
			_ => None,
		};
		match (meta.session(), id) {
			(Some(session), Some(id)) => {
				session.remove_subscription(&self.notification, &id);
				self.unsubscribe.call(id)
			},
			(Some(_), None) => future::err(core::Error::invalid_params("Expected subscription id.")).boxed(),
			_ => future::err(subscriptions_unavailable()).boxed(),
		}
	}
}
