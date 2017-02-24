#![warn(missing_docs)]

extern crate jsonrpc_core as core;
extern crate parking_lot;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use core::futures::{self, sink, future, Sink as FuturesSink, Future, BoxFuture};
use core::futures::sync::{oneshot, mpsc};

type TransportSender = mpsc::Sender<String>;

pub trait PubSubMetadata: core::Metadata {
	fn session(&self) -> Option<Arc<Session>>;
}

pub struct Session {
	active_subscriptions: Mutex<HashMap<(SubscriptionId, String), Box<Fn(SubscriptionId) + Send + 'static>>>,
	transport: Mutex<TransportSender>,
}

impl Session {
	pub fn new(sender: TransportSender) -> Self {
		Session {
			active_subscriptions: Default::default(),
			transport: Mutex::new(sender),
		}
	}

	pub fn add_subscription(&self, name: &str, id: &SubscriptionId, remove: Box<Fn(SubscriptionId) + Send + 'static>) {
		let ret = self.active_subscriptions.lock().insert((id.clone(), name.into()), remove);
		// TODO [ToDr] Should this be a panic?
		assert!(ret.is_none(), "Non-unique subscription id was returned.");
	}

	pub fn remove_subscription(&self, name: &str, id: &SubscriptionId) {
		self.active_subscriptions.lock().remove(&(id.clone(), name.into()));
	}

	pub fn sender(&self) -> TransportSender {
		self.transport.lock().clone()
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionId {
	Number(u64),
	String(String),
}

impl SubscriptionId {
	pub fn parse_value(val: &core::Value) -> Option<SubscriptionId> {
		match *val {
			core::Value::String(ref val) => Some(SubscriptionId::String(val.clone())),
			core::Value::Number(ref val) => val.as_u64().map(SubscriptionId::Number),
			_ => None,
		}
	}
}

impl From<SubscriptionId> for core::Value {
	fn from(sub: SubscriptionId) -> Self {
		match sub {
			SubscriptionId::Number(val) => core::Value::Number(val.into()),
			SubscriptionId::String(val) => core::Value::String(val),
		}
	}
}

pub struct Sink {
	notification: String,
	transport: TransportSender
}

impl Sink {
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

pub struct Subscriber {
	notification: String,
	transport: TransportSender,
	sender: oneshot::Sender<Result<SubscriptionId, core::Error>>,
}

impl Subscriber {
	pub fn assign_id(self, id: SubscriptionId) -> Sink {
		self.sender.complete(Ok(id));

		Sink {
			notification: self.notification,
			transport: self.transport,
		}
	}

	pub fn reject(self, error: core::Error) {
		self.sender.complete(Err(error))
	}
}

pub struct Subscription;
impl Subscription {
	pub fn new<M, F, G>(notification: &str, subscribe: F, unsubscribe: G) -> (Subscribe<F, G>, Unsubscribe<G>) where
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

pub trait SubscribeRpcMethod<M: PubSubMetadata>: Send + Sync + 'static {
	fn call(&self, params: core::Params, meta: M, subscriber: Subscriber);
}

impl<M, F> SubscribeRpcMethod<M> for F where
	F: Fn(core::Params, M, Subscriber) + Send + Sync + 'static,
	M: PubSubMetadata,
{
	fn call(&self, params: core::Params, meta: M, subscriber: Subscriber) {
		(*self)(params, meta, subscriber)
	}
}

pub trait UnsubscribeRpcMethod: Send + Sync + 'static {
	fn call(&self, id: SubscriptionId) -> BoxFuture<core::Value, core::Error>;
}

impl<F> UnsubscribeRpcMethod for F where
	F: Fn(SubscriptionId) -> BoxFuture<core::Value, core::Error> + Send + Sync + 'static,
{
	fn call(&self, id: SubscriptionId) -> BoxFuture<core::Value, core::Error> {
		(*self)(id)
	}
}

pub struct PubSubHandler<T: PubSubMetadata, S: core::Middleware<T> = core::NoopMiddleware> {
	handler: core::MetaIoHandler<T, S>,
}

impl<T: PubSubMetadata, S: core::Middleware<T>> PubSubHandler<T, S> {
	/// Creates new `PubSubHandler`
	pub fn new(handler: core::MetaIoHandler<T, S>) -> Self {
		PubSubHandler {
			handler: handler,
		}
	}

	pub fn add_subscription<F, G>(
		&mut self,
		notification: &str,
		subscribe: (&str, F),
		unsubscribe: (&str, G),
	) where
		F: SubscribeRpcMethod<T>,
		G: UnsubscribeRpcMethod,
	{
		let (sub, unsub) = Subscription::new(notification, subscribe.1, unsubscribe.1);
		self.handler.add_method_with_meta(subscribe.0, sub);
		self.handler.add_method_with_meta(unsubscribe.0, unsub);
	}
}

impl<T: PubSubMetadata, S: core::Middleware<T>> ::std::ops::Deref for PubSubHandler<T, S> {
	type Target = core::MetaIoHandler<T, S>;

	fn deref(&self) -> &Self::Target {
		&self.handler
	}
}

impl<T: PubSubMetadata, S: core::Middleware<T>> ::std::ops::DerefMut for PubSubHandler<T, S> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.handler
	}
}

impl<T: PubSubMetadata, S: core::Middleware<T>> Into<core::MetaIoHandler<T, S>> for PubSubHandler<T, S> {
	fn into(self) -> core::MetaIoHandler<T, S> {
		self.handler
	}
}
