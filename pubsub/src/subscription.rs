//! Subscription primitives.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use crate::core::futures::channel::mpsc;
use crate::core::futures::{
	self, future,
	task::{Context, Poll},
	Future, Sink as FuturesSink, TryFutureExt,
};
use crate::core::{self, BoxFuture};

use crate::handler::{SubscribeRpcMethod, UnsubscribeRpcMethod};
use crate::types::{PubSubMetadata, SinkResult, SubscriptionId, TransportError, TransportSender};

lazy_static::lazy_static! {
	static ref UNSUBSCRIBE_POOL: futures::executor::ThreadPool = futures::executor::ThreadPool::new()
		.expect("Unable to spawn background pool for unsubscribe tasks.");
}

/// RPC client session
/// Keeps track of active subscriptions and unsubscribes from them upon dropping.
pub struct Session {
	active_subscriptions: Mutex<HashMap<(SubscriptionId, String), Box<dyn Fn(SubscriptionId) + Send + 'static>>>,
	transport: TransportSender,
	on_drop: Mutex<Vec<Box<dyn FnMut() + Send>>>,
}

impl fmt::Debug for Session {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		fmt.debug_struct("pubsub::Session")
			.field("active_subscriptions", &self.active_subscriptions.lock().len())
			.field("transport", &self.transport)
			.finish()
	}
}

impl Session {
	/// Creates new session given transport raw send capabilities.
	/// Session should be created as part of metadata, `sender` should be returned by transport.
	pub fn new(sender: TransportSender) -> Self {
		Session {
			active_subscriptions: Default::default(),
			transport: sender,
			on_drop: Default::default(),
		}
	}

	/// Returns transport write stream
	pub fn sender(&self) -> TransportSender {
		self.transport.clone()
	}

	/// Adds a function to call when session is dropped.
	pub fn on_drop<F: FnOnce() + Send + 'static>(&self, on_drop: F) {
		let mut func = Some(on_drop);
		self.on_drop.lock().push(Box::new(move || {
			if let Some(f) = func.take() {
				f();
			}
		}));
	}

	/// Adds new active subscription
	fn add_subscription<F>(&self, name: &str, id: &SubscriptionId, remove: F)
	where
		F: Fn(SubscriptionId) + Send + 'static,
	{
		let ret = self
			.active_subscriptions
			.lock()
			.insert((id.clone(), name.into()), Box::new(remove));
		if let Some(remove) = ret {
			warn!("SubscriptionId collision. Unsubscribing previous client.");
			remove(id.clone());
		}
	}

	/// Removes existing subscription.
	fn remove_subscription(&self, name: &str, id: &SubscriptionId) -> bool {
		self.active_subscriptions
			.lock()
			.remove(&(id.clone(), name.into()))
			.is_some()
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		let mut active = self.active_subscriptions.lock();
		for (id, remove) in active.drain() {
			remove(id.0)
		}

		let mut on_drop = self.on_drop.lock();
		for mut on_drop in on_drop.drain(..) {
			on_drop();
		}
	}
}

/// A handle to send notifications directly to subscribed client.
#[derive(Debug, Clone)]
pub struct Sink {
	notification: String,
	transport: TransportSender,
}

impl Sink {
	/// Sends a notification to a client.
	pub fn notify(&self, val: core::Params) -> SinkResult {
		let val = self.params_to_string(val);
		self.transport.clone().unbounded_send(val)
	}

	fn params_to_string(&self, val: core::Params) -> String {
		let notification = core::Notification {
			jsonrpc: Some(core::Version::V2),
			method: self.notification.clone(),
			params: val,
		};
		core::to_string(&notification).expect("Notification serialization never fails.")
	}
}

impl FuturesSink<core::Params> for Sink {
	type Error = TransportError;

	fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.transport).poll_ready(cx)
	}

	fn start_send(mut self: Pin<&mut Self>, item: core::Params) -> Result<(), Self::Error> {
		let val = self.params_to_string(item);
		Pin::new(&mut self.transport).start_send(val)
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.transport).poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.transport).poll_close(cx)
	}
}

/// Represents a subscribing client.
/// Subscription handlers can either reject this subscription request or assign an unique id.
#[derive(Debug)]
pub struct Subscriber {
	notification: String,
	transport: TransportSender,
	sender: crate::oneshot::Sender<Result<SubscriptionId, core::Error>>,
}

impl Subscriber {
	/// Creates new subscriber.
	///
	/// Should only be used for tests.
	pub fn new_test<T: Into<String>>(
		method: T,
	) -> (
		Self,
		crate::oneshot::Receiver<Result<SubscriptionId, core::Error>>,
		mpsc::UnboundedReceiver<String>,
	) {
		let (sender, id_receiver) = crate::oneshot::channel();
		let (transport, transport_receiver) = mpsc::unbounded();

		let subscriber = Subscriber {
			notification: method.into(),
			transport,
			sender,
		};

		(subscriber, id_receiver, transport_receiver)
	}

	/// Consumes `Subscriber` and assigns unique id to a requestor.
	///
	/// Returns `Err` if request has already terminated.
	pub fn assign_id(self, id: SubscriptionId) -> Result<Sink, ()> {
		let Self {
			notification,
			transport,
			sender,
		} = self;
		sender
			.send(Ok(id))
			.map(|_| Sink {
				notification,
				transport,
			})
			.map_err(|_| ())
	}

	/// Consumes `Subscriber` and assigns unique id to a requestor.
	///
	/// The returned `Future` resolves when the subscriber receives subscription id.
	/// Resolves to `Err` if request has already terminated.
	pub fn assign_id_async(self, id: SubscriptionId) -> impl Future<Output = Result<Sink, ()>> {
		let Self {
			notification,
			transport,
			sender,
		} = self;
		sender.send_and_wait(Ok(id)).map_ok(|_| Sink {
			notification,
			transport,
		})
	}

	/// Rejects this subscription request with given error.
	///
	/// Returns `Err` if request has already terminated.
	pub fn reject(self, error: core::Error) -> Result<(), ()> {
		self.sender.send(Err(error)).map_err(|_| ())
	}

	/// Rejects this subscription request with given error.
	///
	/// The returned `Future` resolves when the rejection is sent to the client.
	/// Resolves to `Err` if request has already terminated.
	pub fn reject_async(self, error: core::Error) -> impl Future<Output = Result<(), ()>> {
		self.sender.send_and_wait(Err(error)).map_ok(|_| ()).map_err(|_| ())
	}
}

/// Creates new subscribe and unsubscribe RPC methods
pub fn new_subscription<M, F, G>(notification: &str, subscribe: F, unsubscribe: G) -> (Subscribe<F, G>, Unsubscribe<G>)
where
	M: PubSubMetadata,
	F: SubscribeRpcMethod<M>,
	G: UnsubscribeRpcMethod<M>,
{
	let unsubscribe = Arc::new(unsubscribe);
	let subscribe = Subscribe {
		notification: notification.to_owned(),
		unsubscribe: unsubscribe.clone(),
		subscribe,
	};

	let unsubscribe = Unsubscribe {
		notification: notification.into(),
		unsubscribe,
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

impl<M, F, G> core::RpcMethod<M> for Subscribe<F, G>
where
	M: PubSubMetadata,
	F: SubscribeRpcMethod<M>,
	G: UnsubscribeRpcMethod<M>,
{
	fn call(&self, params: core::Params, meta: M) -> BoxFuture<core::Result<core::Value>> {
		match meta.session() {
			Some(session) => {
				let (tx, rx) = crate::oneshot::channel();

				// Register the subscription
				let subscriber = Subscriber {
					notification: self.notification.clone(),
					transport: session.sender(),
					sender: tx,
				};
				self.subscribe.call(params, meta, subscriber);

				let unsub = self.unsubscribe.clone();
				let notification = self.notification.clone();
				let subscribe_future = rx.map_err(|_| subscription_rejected()).and_then(move |result| {
					futures::future::ready(match result {
						Ok(id) => {
							session.add_subscription(&notification, &id, move |id| {
								// TODO [#570] [ToDr] We currently run unsubscribe tasks on a shared thread pool.
								// In the future we should use some kind of `::spawn` method
								// that spawns a task on an existing executor or pass the spawner handle here.
								let f = unsub.call(id, None);
								UNSUBSCRIBE_POOL.spawn_ok(async move {
									let _ = f.await;
								});
							});
							Ok(id.into())
						}
						Err(e) => Err(e),
					})
				});
				Box::pin(subscribe_future)
			}
			None => Box::pin(future::err(subscriptions_unavailable())),
		}
	}
}

/// Unsubscribe RPC implementation.
pub struct Unsubscribe<G> {
	notification: String,
	unsubscribe: Arc<G>,
}

impl<M, G> core::RpcMethod<M> for Unsubscribe<G>
where
	M: PubSubMetadata,
	G: UnsubscribeRpcMethod<M>,
{
	fn call(&self, params: core::Params, meta: M) -> BoxFuture<core::Result<core::Value>> {
		let id = match params {
			core::Params::Array(ref vec) if vec.len() == 1 => SubscriptionId::parse_value(&vec[0]),
			_ => None,
		};
		match (meta.session(), id) {
			(Some(session), Some(id)) => {
				if session.remove_subscription(&self.notification, &id) {
					Box::pin(self.unsubscribe.call(id, Some(meta)))
				} else {
					Box::pin(future::err(core::Error::invalid_params("Invalid subscription id.")))
				}
			}
			(Some(_), None) => Box::pin(future::err(core::Error::invalid_params("Expected subscription id."))),
			_ => Box::pin(future::err(subscriptions_unavailable())),
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::core;
	use crate::core::futures::channel::mpsc;
	use crate::core::RpcMethod;
	use crate::types::{PubSubMetadata, SubscriptionId};
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::sync::Arc;

	use super::{new_subscription, Session, Sink, Subscriber};

	fn session() -> (Session, mpsc::UnboundedReceiver<String>) {
		let (tx, rx) = mpsc::unbounded();
		(Session::new(tx), rx)
	}

	#[test]
	fn should_unregister_on_drop() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let session = session().0;
		session.add_subscription("test", &id, move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		});

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
		session.add_subscription("test", &id, move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		});

		// when
		let removed = session.remove_subscription("test", &id);
		drop(session);

		// then
		assert_eq!(removed, true);
		assert_eq!(called.load(Ordering::SeqCst), false);
	}

	#[test]
	fn should_not_remove_subscription_if_invalid() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let other_session = session().0;
		let session = session().0;
		session.add_subscription("test", &id, move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		});

		// when
		let removed = other_session.remove_subscription("test", &id);
		drop(session);

		// then
		assert_eq!(removed, false);
		assert_eq!(called.load(Ordering::SeqCst), true);
	}

	#[test]
	fn should_unregister_in_case_of_collision() {
		// given
		let id = SubscriptionId::Number(1);
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		let session = session().0;
		session.add_subscription("test", &id, move |id| {
			assert_eq!(id, SubscriptionId::Number(1));
			called2.store(true, Ordering::SeqCst);
		});

		// when
		session.add_subscription("test", &id, |_| {});

		// then
		assert_eq!(called.load(Ordering::SeqCst), true);
	}

	#[test]
	fn should_send_notification_to_the_transport() {
		// given
		let (tx, mut rx) = mpsc::unbounded();
		let sink = Sink {
			notification: "test".into(),
			transport: tx,
		};

		// when
		sink.notify(core::Params::Array(vec![core::Value::Number(10.into())]))
			.unwrap();

		let val = rx.try_next().unwrap();
		// then
		assert_eq!(val, Some(r#"{"jsonrpc":"2.0","method":"test","params":[10]}"#.into()));
	}

	#[test]
	fn should_assign_id() {
		// given
		let (transport, _) = mpsc::unbounded();
		let (tx, rx) = crate::oneshot::channel();
		let subscriber = Subscriber {
			notification: "test".into(),
			transport,
			sender: tx,
		};

		// when
		let sink = subscriber.assign_id_async(SubscriptionId::Number(5));

		// then
		futures::executor::block_on(async move {
			let id = rx.await;
			assert_eq!(id, Ok(Ok(SubscriptionId::Number(5))));
			let sink = sink.await.unwrap();
			assert_eq!(sink.notification, "test".to_owned());
		})
	}

	#[test]
	fn should_reject() {
		// given
		let (transport, _) = mpsc::unbounded();
		let (tx, rx) = crate::oneshot::channel();
		let subscriber = Subscriber {
			notification: "test".into(),
			transport,
			sender: tx,
		};
		let error = core::Error {
			code: core::ErrorCode::InvalidRequest,
			message: "Cannot start subscription now.".into(),
			data: None,
		};

		// when
		let reject = subscriber.reject_async(error.clone());

		// then
		futures::executor::block_on(async move {
			assert_eq!(rx.await.unwrap(), Err(error));
			reject.await.unwrap();
		});
	}

	#[derive(Clone)]
	struct Metadata(Arc<Session>);
	impl core::Metadata for Metadata {}
	impl PubSubMetadata for Metadata {
		fn session(&self) -> Option<Arc<Session>> {
			Some(self.0.clone())
		}
	}
	impl Default for Metadata {
		fn default() -> Self {
			Self(Arc::new(session().0))
		}
	}

	#[test]
	fn should_subscribe() {
		// given
		let (subscribe, _) = new_subscription(
			"test".into(),
			move |params, _meta, subscriber: Subscriber| {
				assert_eq!(params, core::Params::None);
				let _sink = subscriber.assign_id(SubscriptionId::Number(5)).unwrap();
			},
			|_id, _meta| async { Ok(core::Value::Bool(true)) },
		);

		// when
		let meta = Metadata::default();
		let result = subscribe.call(core::Params::None, meta);

		// then
		assert_eq!(futures::executor::block_on(result), Ok(serde_json::json!(5)));
	}

	#[test]
	fn should_unsubscribe() {
		// given
		const SUB_ID: u64 = 5;
		let (subscribe, unsubscribe) = new_subscription(
			"test".into(),
			move |params, _meta, subscriber: Subscriber| {
				assert_eq!(params, core::Params::None);
				let _sink = subscriber.assign_id(SubscriptionId::Number(SUB_ID)).unwrap();
			},
			|_id, _meta| async { Ok(core::Value::Bool(true)) },
		);

		// when
		let meta = Metadata::default();
		futures::executor::block_on(subscribe.call(core::Params::None, meta.clone())).unwrap();
		let result = unsubscribe.call(core::Params::Array(vec![serde_json::json!(SUB_ID)]), meta);

		// then
		assert_eq!(futures::executor::block_on(result), Ok(serde_json::json!(true)));
	}

	#[test]
	fn should_not_unsubscribe_if_invalid() {
		// given
		const SUB_ID: u64 = 5;
		let (subscribe, unsubscribe) = new_subscription(
			"test".into(),
			move |params, _meta, subscriber: Subscriber| {
				assert_eq!(params, core::Params::None);
				let _sink = subscriber.assign_id(SubscriptionId::Number(SUB_ID)).unwrap();
			},
			|_id, _meta| async { Ok(core::Value::Bool(true)) },
		);

		// when
		let meta = Metadata::default();
		futures::executor::block_on(subscribe.call(core::Params::None, meta.clone())).unwrap();
		let result = unsubscribe.call(core::Params::Array(vec![serde_json::json!(SUB_ID + 1)]), meta);

		// then
		assert_eq!(
			futures::executor::block_on(result),
			Err(core::Error {
				code: core::ErrorCode::InvalidParams,
				message: "Invalid subscription id.".into(),
				data: None,
			})
		);
	}
}
