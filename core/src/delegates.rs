//! Delegate rpc calls

use std::collections::HashMap;
use std::sync::Arc;

use crate::calls::{Metadata, RemoteProcedure, RpcMethod, RpcNotification};
use crate::types::{Error, Params, Value};
use crate::BoxFuture;
use futures::IntoFuture;

struct DelegateAsyncMethod<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F, I> RpcMethod<M> for DelegateAsyncMethod<T, F>
where
	M: Metadata,
	F: Fn(&T, Params) -> I,
	I: IntoFuture<Item = Value, Error = Error>,
	T: Send + Sync + 'static,
	F: Send + Sync + 'static,
	I::Future: Send + 'static,
{
	fn call(&self, params: Params, _meta: M) -> BoxFuture<Value> {
		let closure = &self.closure;
		Box::new(closure(&self.delegate, params).into_future())
	}
}

struct DelegateMethodWithMeta<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F, I> RpcMethod<M> for DelegateMethodWithMeta<T, F>
where
	M: Metadata,
	F: Fn(&T, Params, M) -> I,
	I: IntoFuture<Item = Value, Error = Error>,
	T: Send + Sync + 'static,
	F: Send + Sync + 'static,
	I::Future: Send + 'static,
{
	fn call(&self, params: Params, meta: M) -> BoxFuture<Value> {
		let closure = &self.closure;
		Box::new(closure(&self.delegate, params, meta).into_future())
	}
}

struct DelegateNotification<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> RpcNotification<M> for DelegateNotification<T, F>
where
	M: Metadata,
	F: Fn(&T, Params) + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
{
	fn execute(&self, params: Params, _meta: M) {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

struct DelegateNotificationWithMeta<T, F> {
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> RpcNotification<M> for DelegateNotificationWithMeta<T, F>
where
	M: Metadata,
	F: Fn(&T, Params, M) + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
{
	fn execute(&self, params: Params, meta: M) {
		let closure = &self.closure;
		closure(&self.delegate, params, meta)
	}
}

/// A set of RPC methods and notifications tied to single `delegate` struct.
pub struct IoDelegate<T, M = ()>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	delegate: Arc<T>,
	methods: HashMap<String, RemoteProcedure<M>>,
}

impl<T, M> IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	/// Creates new `IoDelegate`
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			delegate,
			methods: HashMap::new(),
		}
	}

	/// Adds an alias to existing method.
	/// NOTE: Aliases are not transitive, i.e. you cannot create alias to an alias.
	pub fn add_alias(&mut self, from: &str, to: &str) {
		self.methods.insert(from.into(), RemoteProcedure::Alias(to.into()));
	}

	/// Adds async method to the delegate.
	pub fn add_method<F, I>(&mut self, name: &str, method: F)
	where
		F: Fn(&T, Params) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		F: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Method(Arc::new(DelegateAsyncMethod {
				delegate: self.delegate.clone(),
				closure: method,
			})),
		);
	}

	/// Adds async method with metadata to the delegate.
	pub fn add_method_with_meta<F, I>(&mut self, name: &str, method: F)
	where
		F: Fn(&T, Params, M) -> I,
		I: IntoFuture<Item = Value, Error = Error>,
		F: Send + Sync + 'static,
		I::Future: Send + 'static,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Method(Arc::new(DelegateMethodWithMeta {
				delegate: self.delegate.clone(),
				closure: method,
			})),
		);
	}

	/// Adds notification to the delegate.
	pub fn add_notification<F>(&mut self, name: &str, notification: F)
	where
		F: Fn(&T, Params),
		F: Send + Sync + 'static,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Notification(Arc::new(DelegateNotification {
				delegate: self.delegate.clone(),
				closure: notification,
			})),
		);
	}

	/// Adds notification with metadata to the delegate.
	pub fn add_notification_with_meta<F>(&mut self, name: &str, notification: F)
	where
		F: Fn(&T, Params, M),
		F: Send + Sync + 'static,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Notification(Arc::new(DelegateNotificationWithMeta {
				delegate: self.delegate.clone(),
				closure: notification,
			})),
		);
	}
}

impl<T, M> crate::io::IoHandlerExtension<M> for IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn augment<S: crate::Middleware<M>>(self, handler: &mut crate::MetaIoHandler<M, S>) {
		handler.extend_with(self.methods)
	}
}

impl<T, M> IntoIterator for IoDelegate<T, M>
where
	T: Send + Sync + 'static,
	M: Metadata,
{
	type Item = (String, RemoteProcedure<M>);
	type IntoIter = std::collections::hash_map::IntoIter<String, RemoteProcedure<M>>;

	fn into_iter(self) -> Self::IntoIter {
		self.methods.into_iter()
	}
}
