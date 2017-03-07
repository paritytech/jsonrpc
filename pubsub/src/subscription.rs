//! Subscription primitives.

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
	transport: TransportSender,
}

impl Session {
	/// Creates new session given transport raw send capabilities.
	/// Session should be created as part of metadata, `sender` should be returned by transport.
	pub fn new(sender: TransportSender) -> Self {
		Session {
			active_subscriptions: Default::default(),
			transport: sender,
		}
	}

	/// Returns transport write stream
	pub fn sender(&self) -> TransportSender {
		self.transport.clone()
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

				// Register the subscription
				let subscriber = Subscriber {
					notification: self.notification.clone(),
					transport: session.sender(),
					sender: tx,
				};
				self.subscribe.call(params, meta, subscriber);

				let unsub = self.unsubscribe.clone();
				let notification = self.notification.clone();
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

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};
	use core;
	use core::RpcMethod;
	use core::futures::{future, Async, Future, Stream};
	use core::futures::sync::{mpsc, oneshot};
	use types::{SubscriptionId, PubSubMetadata};

	use super::{Session, Sink, Subscriber, new_subscription};

	fn session() -> (Session, mpsc::Receiver<String>) {
		let (tx, rx) = mpsc::channel(1);
		(Session::new(tx), rx)
	}

	#[test]
	fn should_unregister_on_drop() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let session = session().0;
		session.add_subscription("test", &id, Box::new(move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		}));

		// when
		drop(session);

		// then
		assert_eq!(called.load(Ordering::SeqCst), true);
	}

	#[test]
	fn should_remove_subscription() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let session = session().0;
		session.add_subscription("test", &id, Box::new(move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		}));

		// when
		session.remove_subscription("test", &id);
		drop(session);

		// then
		assert_eq!(called.load(Ordering::SeqCst), false);
	}

	#[test]
	fn should_unregister_in_case_of_collision() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let session = session().0;
		session.add_subscription("test", &id, Box::new(move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		}));

		// when
		session.add_subscription("test", &id, Box::new(|_| {}));

		// then
		assert_eq!(called.load(Ordering::SeqCst), true);
	}

	#[test]
	fn should_send_notification_to_the_transport() {
		// given
		let (tx, mut rx) = mpsc::channel(1);
		let sink = Sink {
			notification: "test".into(),
			transport: tx,
		};

		// when
		sink.send(core::Params::Array(vec![core::Value::Number(10.into())])).wait().unwrap();

		// then
		assert_eq!(
			rx.poll().unwrap(),
			Async::Ready(Some(r#"{"jsonrpc":"2.0","method":"test","params":[10]}"#.into()))
		);
	}

	#[test]
	fn should_assign_id() {
		// given
		let (transport, _) = mpsc::channel(1);
		let (tx, mut rx) = oneshot::channel();
		let subscriber = Subscriber {
			notification: "test".into(),
			transport: transport,
			sender: tx,
		};

		// when
		let sink = subscriber.assign_id(SubscriptionId::Number(5));

		// then
		assert_eq!(
			rx.poll().unwrap(),
			Async::Ready(Ok(SubscriptionId::Number(5)))
		);
		assert_eq!(sink.notification, "test".to_owned());
	}

	#[test]
	fn should_reject() {
		// given
		let (transport, _) = mpsc::channel(1);
		let (tx, mut rx) = oneshot::channel();
		let subscriber = Subscriber {
			notification: "test".into(),
			transport: transport,
			sender: tx,
		};
		let error = core::Error {
			code: core::ErrorCode::InvalidRequest,
			message: "Cannot start subscription now.".into(),
			data: None,
		};

		// when
		subscriber.reject(error.clone());

		// then
		assert_eq!(
			rx.poll().unwrap(),
			Async::Ready(Err(error))
		);
	}

	#[derive(Clone, Default)]
	struct Metadata;
	impl core::Metadata for Metadata {}
	impl PubSubMetadata for Metadata {
		fn session(&self) -> Option<Arc<Session>> {
			Some(Arc::new(session().0))
		}
	}

	#[test]
	fn should_subscribe() {
		// given
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let (subscribe, _) = new_subscription("test".into(), move |params, _meta, _subscriber| {
			assert_eq!(params, core::Params::None);
			called2.store(true, Ordering::SeqCst);
		}, |_id| future::ok(core::Value::Bool(true)).boxed());
		let meta = Metadata;

		// when
		let result = subscribe.call(core::Params::None, meta);

		// then
		assert_eq!(called.load(Ordering::SeqCst), true);
		assert_eq!(result.wait(), Err(core::Error {
			code: core::ErrorCode::ServerError(-32091),
			message: "Subscription rejected".into(),
			data: None,
		}));
	}
}
