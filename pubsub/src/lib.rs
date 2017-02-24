extern crate jsonrpc_core as core;

use core::futures::BoxFuture;
use core::futures::sync::oneshot::Sender;

pub trait PubSubMetadata: core::Metadata {
	fn send(data: String);
}

pub type SubscriptionId = core::Value;

pub struct Sink<M: PubSubMetadata> {
	meta: M
}

impl<M: PubSubMetadata> Sink<M> {
	pub fn send(&self, val: core::Value) {
		unimplemented!()
	}
}

pub struct Subscriber<M: PubSubMetadata> {
	meta: M,
	sender: Sender<Result<SubscriptionId, core::Error>>,
}

impl<M: PubSubMetadata> Subscriber<M> {
	pub fn assign_id(self, id: SubscriptionId) -> Sink<M> {
		unimplemented!()
	}

	pub fn reject(self, error: core::Error) {
		unimplemented!()
	}
}

impl<M: PubSubMetadata> Drop for Subscriber<M> {
	fn drop(&mut self) {
		// Reject
		unimplemented!()
	}
}

pub trait SubscribeRpcMethod<M: PubSubMetadata> {
	fn call(&self, params: core::Params, meta: M, subscriber: Subscriber<M>);
}

impl<M, F> SubscribeRpcMethod<M> for F where
	F: Fn(core::Params, M, Subscriber<M>),
	M: PubSubMetadata,
{
	fn call(&self, params: core::Params, meta: M, subscriber: Subscriber<M>) {
		(*self)(params, meta, subscriber)
	}
}

pub trait UnsubscribeRpcMethod {
	fn call(&self, id: SubscriptionId) -> BoxFuture<bool, core::Error>;
}

impl<F> UnsubscribeRpcMethod for F where
	F: Fn(SubscriptionId) -> BoxFuture<bool, core::Error>,
{
	fn call(&self, id: SubscriptionId) -> BoxFuture<bool, core::Error> {
		(*self)(id)
	}
}

pub struct PubSubHandler<T: PubSubMetadata, S: core::Middleware<T> = core::NoopMiddleware> {
	handler: core::MetaIoHandler<T, S>,
}

impl<T: PubSubMetadata> Default for PubSubHandler<T, core::NoopMiddleware> {
	fn default() -> Self {
		PubSubHandler {
			handler: Default::default(),
		}
	}
}

impl<T: PubSubMetadata> PubSubHandler<T, core::NoopMiddleware> {
	/// Creates new `PubSubHandler` compatible with specified protocol version.
	pub fn with_compatibility(compatibility: core::Compatibility) -> Self {
		PubSubHandler {
			handler: core::MetaIoHandler::with_compatibility(compatibility),
		}
	}
}

impl<T: PubSubMetadata, S: core::Middleware<T>> PubSubHandler<T, S> {
	/// Creates new `PubSubHandler`
	pub fn new(compatibility: core::Compatibility, middleware: S) -> Self {
		PubSubHandler {
			handler: core::MetaIoHandler::new(compatibility, middleware),
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
		unimplemented!()
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
