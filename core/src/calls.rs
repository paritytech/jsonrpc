use std::sync::Arc;
use types::{Params, Value, Error};
use futures::{BoxFuture, Future};

/// Metadata trait
pub trait Metadata: Default + Clone + Send + 'static {}
impl Metadata for () {}

/// Synchronous Method
pub trait RpcMethodSync: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params) -> Result<Value, Error>;
}

/// Asynchronous Method
pub trait RpcMethodSimple: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params) -> BoxFuture<Value, Error>;
}

/// Asynchronous Method with Metadata
pub trait RpcMethod<T: Metadata>: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error>;
}

/// Notification
pub trait RpcNotificationSimple: Send + Sync + 'static {
	/// Execute notification
	fn execute(&self, params: Params);
}

/// Notification with Metadata
pub trait RpcNotification<T: Metadata>: Send + Sync + 'static {
	/// Execute notification
	fn execute(&self, params: Params, meta: T);
}

/// Possible Remote Procedures with Metadata
#[derive(Clone)]
pub enum RemoteProcedure<T: Metadata> {
	/// A method call
	Method(Arc<RpcMethod<T>>),
	/// A notification
	Notification(Arc<RpcNotification<T>>),
	/// An alias to other method,
	Alias(String),
}

impl<F: Send + Sync + 'static> RpcMethodSync for F where
	F: Fn(Params) -> Result<Value, Error>,
{
	fn call(&self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static> RpcMethodSimple for F where
	F: Fn(Params) -> X,
	X: Future<Item=Value, Error=Error>,
{
	fn call(&self, params: Params) -> BoxFuture<Value, Error> {
		self(params).boxed()
	}
}

impl<F: Send + Sync + 'static> RpcNotificationSimple for F where
	F: Fn(Params),
{
	fn execute(&self, params: Params) {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static, T> RpcMethod<T> for F where
	T: Metadata,
	F: Fn(Params, T) -> X,
	X: Future<Item=Value, Error=Error>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error> {
		self(params, meta).boxed()
	}
}

impl<F: Send + Sync + 'static, T> RpcNotification<T> for F where
	T: Metadata,
	F: Fn(Params, T),
{
	fn execute(&self, params: Params, meta: T) {
		self(params, meta)
	}
}
