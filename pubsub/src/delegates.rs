use std::marker::PhantomData;
use std::sync::Arc;

use crate::core::futures::IntoFuture;
use crate::core::{self, Error, Metadata, Params, RemoteProcedure, RpcMethod, Value};
use crate::handler::{SubscribeRpcMethod, UnsubscribeRpcMethod};
use crate::subscription::{new_subscription, Subscriber};
use crate::types::{PubSubMetadata, SubscriptionId};

struct DelegateSubscription<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> SubscribeRpcMethod<M> for DelegateSubscription<T, F>
where
	M: PubSubMetadata,
	F: Fn(&T, Params, M, Subscriber),
	T: Send + Sync + 'static,
	F: Send + Sync + 'static,
{
	fn call(&self, params: Params, meta: M, subscriber: Subscriber) {
		let closure = &self.closure;
		closure(&self.delegate, params, meta, subscriber)
	}
}

impl<M, T, F, I> UnsubscribeRpcMethod<M> for DelegateSubscription<T, F>
where
	M: PubSubMetadata,
	F: Fn(&T, SubscriptionId, Option<M>) -> I,
	I: IntoFuture<Item = Value, Error = Error>,
	T: Send + Sync + 'static,
	F: Send + Sync + 'static,
	I::Future: Send + 'static,
{
	type Out = I::Future;
	fn call(&self, id: SubscriptionId, meta: Option<M>) -> Self::Out {
		let closure = &self.closure;
		closure(&self.delegate, id, meta).into_future()
	}
}

/// Wire up rpc subscriptions to `delegate` struct
pub struct IoDelegate<T, M = ()>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	inner: core::IoDelegate<T, M>,
	delegate: Arc<T>,
	_data: PhantomData<M>,
}

impl<T, M> IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: PubSubMetadata,
{
	/// Creates new `PubSubIoDelegate`, wrapping the core IoDelegate
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			inner: core::IoDelegate::new(delegate.clone()),
			delegate,
			_data: PhantomData,
		}
	}

	/// Adds subscription to the delegate.
	pub fn add_subscription<Sub, Unsub, I>(&mut self, name: &str, subscribe: (&str, Sub), unsubscribe: (&str, Unsub))
	where
		Sub: Fn(&T, Params, M, Subscriber),
		Sub: Send + Sync + 'static,
		Unsub: Fn(&T, SubscriptionId, Option<M>) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		Unsub: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		let (sub, unsub) = new_subscription(
			name,
			DelegateSubscription {
				delegate: self.delegate.clone(),
				closure: subscribe.1,
			},
			DelegateSubscription {
				delegate: self.delegate.clone(),
				closure: unsubscribe.1,
			},
		);
		self.inner
			.add_method_with_meta(subscribe.0, move |_, params, meta| sub.call(params, meta));
		self.inner
			.add_method_with_meta(unsubscribe.0, move |_, params, meta| unsub.call(params, meta));
	}

	/// Adds an alias to existing method.
	pub fn add_alias(&mut self, from: &str, to: &str) {
		self.inner.add_alias(from, to)
	}

	/// Adds async method to the delegate.
	pub fn add_method<F, I>(&mut self, name: &str, method: F)
	where
		F: Fn(&T, Params) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		F: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		self.inner.add_method(name, method)
	}

	/// Adds async method with metadata to the delegate.
	pub fn add_method_with_meta<F, I>(&mut self, name: &str, method: F)
	where
		F: Fn(&T, Params, M) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		F: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		self.inner.add_method_with_meta(name, method)
	}

	/// Adds notification to the delegate.
	pub fn add_notification<F>(&mut self, name: &str, notification: F)
	where
		F: Fn(&T, Params),
		F: Send + Sync + 'static,
	{
		self.inner.add_notification(name, notification)
	}
}

impl<T, M> core::IoHandlerExtension<M> for IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn augment<S: core::Middleware<M>>(self, handler: &mut core::MetaIoHandler<M, S>) {
		handler.extend_with(self.inner)
	}
}

impl<T, M> IntoIterator for IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	type Item = (String, RemoteProcedure<M>);
	type IntoIter = <core::IoDelegate<T, M> as IntoIterator>::IntoIter;

	fn into_iter(self) -> Self::IntoIter {
		self.inner.into_iter()
	}
}
