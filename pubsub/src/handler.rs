use core;
use core::futures::BoxFuture;

use types::{PubSubMetadata, SubscriptionId};
use subscription::{Subscriber, new_subscription};

/// Subscribe handler
pub trait SubscribeRpcMethod<M: PubSubMetadata>: Send + Sync + 'static {
	/// Called when client is requesting new subscription to be started.
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

/// Unsubscribe handler
pub trait UnsubscribeRpcMethod: Send + Sync + 'static {
	/// Called when client is requesting to cancel existing subscription.
	fn call(&self, id: SubscriptionId) -> BoxFuture<core::Value, core::Error>;
}

impl<F> UnsubscribeRpcMethod for F where
	F: Fn(SubscriptionId) -> BoxFuture<core::Value, core::Error> + Send + Sync + 'static,
{
	fn call(&self, id: SubscriptionId) -> BoxFuture<core::Value, core::Error> {
		(*self)(id)
	}
}

/// Publish-Subscribe extension of `IoHandler`.
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

	/// Adds new subscription.
	pub fn add_subscription<F, G>(
		&mut self,
		notification: &str,
		subscribe: (&str, F),
		unsubscribe: (&str, G),
	) where
		F: SubscribeRpcMethod<T>,
		G: UnsubscribeRpcMethod,
	{
		let (sub, unsub) = new_subscription(notification, subscribe.1, unsubscribe.1);
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
