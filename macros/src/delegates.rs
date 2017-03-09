use std::sync::Arc;
use std::collections::HashMap;

use jsonrpc_core::{Params, Value, Error};
use jsonrpc_core::{Metadata, RemoteProcedure, RpcMethod, RpcNotification};
use jsonrpc_core::futures::{self, BoxFuture, Future};

type Data = Result<Value, Error>;
type AsyncData = BoxFuture<Value, Error>;

struct DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Data + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
{
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> RpcMethod<M> for DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Data + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn call(&self, params: Params, _meta: M) -> AsyncData {
		let closure = &self.closure;
		futures::done(closure(&self.delegate, params)).boxed()
	}
}

struct DelegateAsyncMethod<T, F> where
	F: Fn(&T, Params) -> AsyncData,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
{
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> RpcMethod<M> for DelegateAsyncMethod<T, F> where
	F: Fn(&T, Params) -> AsyncData,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn call(&self, params: Params, _meta: M) -> AsyncData {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

struct DelegateMethodWithMeta<T, M> where
	T: Send + Sync + 'static,
	M: Metadata,
{
	delegate: Arc<T>,
	closure: Box<Fn(&T, Params, M) -> AsyncData + Send + Sync>,
}

impl<T, M> RpcMethod<M> for DelegateMethodWithMeta<T, M> where
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn call(&self, params: Params, meta: M) -> AsyncData {
		let closure = &self.closure;
		closure(&self.delegate, params, meta)
	}
}

struct DelegateNotification<T, F> where
	F: Fn(&T, Params) + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
{
	delegate: Arc<T>,
	closure: F,
}

impl<T, M, F> RpcNotification<M> for DelegateNotification<T, F> where
	F: Fn(&T, Params) + 'static,
	F: Send + Sync + 'static,
	T: Send + Sync + 'static,
	M: Metadata,
{
	fn execute(&self, params: Params, _meta: M) {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

/// A set of RPC methods and notifications tied to single `delegate` struct.
pub struct IoDelegate<T, M = ()> where
	T: Send + Sync + 'static,
	M: Metadata,
{
	delegate: Arc<T>,
	methods: HashMap<String, RemoteProcedure<M>>,
}

impl<T, M> IoDelegate<T, M> where
	T: Send + Sync + 'static,
	M: Metadata,
{
	/// Creates new `IoDelegate`
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			delegate: delegate,
			methods: HashMap::new(),
		}
	}

	pub fn add_alias(&mut self, from: &str, to: &str) {
		self.methods.insert(from.into(), RemoteProcedure::Alias(to.into()));
	}

	pub fn add_method<F>(&mut self, name: &str, method: F) where
		F: Fn(&T, Params) -> Data,
		F: Send + Sync + 'static,
	{
		self.methods.insert(name.into(), RemoteProcedure::Method(Box::new(
			DelegateMethod {
				delegate: self.delegate.clone(),
				closure: method,
			}
		)));
	}

	pub fn add_async_method<F>(&mut self, name: &str, method: F) where
		F: Fn(&T, Params) -> AsyncData,
		F: Send + Sync + 'static,
	{
		self.methods.insert(name.into(), RemoteProcedure::Method(Box::new(
			DelegateAsyncMethod {
				delegate: self.delegate.clone(),
				closure: method,
			}
		)));
	}

	pub fn add_method_with_meta<F>(&mut self, name: &str, method: F) where
		F: Fn(&T, Params, M) -> AsyncData,
		F: Send + Sync + 'static,
	{
		self.methods.insert(name.into(), RemoteProcedure::Method(Box::new(
			DelegateMethodWithMeta {
				delegate: self.delegate.clone(),
				closure: Box::new(method),
			}
		)));
	}

	pub fn add_notification<F>(&mut self, name: &str, notification: F) where
		F: Fn(&T, Params),
		F: Send + Sync + 'static,
	{
		self.methods.insert(name.into(), RemoteProcedure::Notification(Box::new(
			DelegateNotification {
				delegate: self.delegate.clone(),
				closure: notification,
			}
		)));
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
