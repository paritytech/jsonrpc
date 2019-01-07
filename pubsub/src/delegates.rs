use std::sync::Arc;
use std::collections::HashMap;

use types::{Params, Value, Error};
use calls::{BoxFuture, Metadata, RemoteProcedure, RpcMethod, RpcNotification};
use futures::IntoFuture;

use jsonrpc_pubsub::{self, SubscriptionId, Subscriber, PubSubMetadata};

impl<T, M, F> jsonrpc_pubsub::SubscribeRpcMethod<M> for DelegateSubscribe<T, F> where
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

struct DelegateUnsubscribe<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<M, T, F, I> jsonrpc_pubsub::UnsubscribeRpcMethod<M> for DelegateUnsubscribe<T, F> where
	M: PubSubMetadata,
	F: Fn(&T, SubscriptionId, M) -> I,
	I: IntoFuture<Item = Value, Error = Error>,
	T: Send + Sync + 'static,
	F: Send + Sync + 'static,
	I::Future: Send + 'static,
{
	type Out = I::Future;
	fn call(&self, id: SubscriptionId, meta: M) -> Self::Out {
		let closure = &self.closure;
		closure(&self.delegate, id, meta).into_future()
	}
}

impl<T, M> IoDelegate<T, M> where
	T: Send + Sync + 'static,
	M: PubSubMetadata,
{
	/// Adds subscription to the delegate.
	pub fn add_subscription<Sub, Unsub, I>(
		&mut self,
		name: &str,
		subscribe: (&str, Sub),
		unsubscribe: (&str, Unsub),
	) where
		Sub: Fn(&T, Params, M, Subscriber),
		Sub: Send + Sync + 'static,
		Unsub: Fn(&T, SubscriptionId, M) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		Unsub: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		let (sub, unsub) = jsonrpc_pubsub::new_subscription(
			name,
			DelegateSubscribe {
				delegate: self.delegate.clone(),
				closure: subscribe.1,
			},
			DelegateUnsubscribe {
				delegate: self.delegate.clone(),
				closure: unsubscribe.1,
			}
		);
		self.add_method_with_meta(subscribe.0, move |_, params, meta| sub.call(params, meta));
		self.add_method_with_meta(unsubscribe.0, move |_, params, meta| unsub.call(params, meta));
	}
}

impl<T, M> Into<HashMap<String, RemoteProcedure<M>>> for IoDelegate<T, M> where
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn into(self) -> HashMap<String, RemoteProcedure<M>> {
		self.methods
	}
}
