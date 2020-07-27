use crate::types::{Error, Params, Value};
use crate::BoxFuture;
use futures::Future;
use std::fmt;
use std::sync::Arc;

/// Metadata trait
pub trait Metadata: Clone + Send + 'static {}
impl Metadata for () {}
impl<T: Metadata> Metadata for Option<T> {}
impl<T: Metadata> Metadata for Box<T> {}
impl<T: Sync + Send + 'static> Metadata for Arc<T> {}

/// A future-conversion trait.
pub trait WrapFuture<T, E> {
	/// Convert itself into a boxed future.
	fn into_future(self) -> BoxFuture<Result<T, E>>;
}

impl<T: Send + 'static, E: Send + 'static> WrapFuture<T, E> for Result<T, E> {
	fn into_future(self) -> BoxFuture<Result<T, E>> {
		Box::pin(futures::future::ready(self))
	}
}

impl<T, E> WrapFuture<T, E> for BoxFuture<Result<T, E>> {
	fn into_future(self) -> BoxFuture<Result<T, E>> {
		self
	}
}

/// A synchronous or asynchronous method.
pub trait RpcMethodSync: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params) -> BoxFuture<crate::Result<Value>>;
}

/// Asynchronous Method
pub trait RpcMethodSimple: Send + Sync + 'static {
	/// Output future
	type Out: Future<Output = Result<Value, Error>> + Send;
	/// Call method
	fn call(&self, params: Params) -> Self::Out;
}

/// Asynchronous Method with Metadata
pub trait RpcMethod<T: Metadata>: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params, meta: T) -> BoxFuture<crate::Result<Value>>;
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
	Method(Arc<dyn RpcMethod<T>>),
	/// A notification
	Notification(Arc<dyn RpcNotification<T>>),
	/// An alias to other method,
	Alias(String),
}

impl<T: Metadata> fmt::Debug for RemoteProcedure<T> {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		use self::RemoteProcedure::*;
		match *self {
			Method(..) => write!(fmt, "<method>"),
			Notification(..) => write!(fmt, "<notification>"),
			Alias(ref alias) => write!(fmt, "alias => {:?}", alias),
		}
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static> RpcMethodSimple for F
where
	F: Fn(Params) -> X,
	X: Future<Output = Result<Value, Error>>,
{
	type Out = X;
	fn call(&self, params: Params) -> Self::Out {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static> RpcMethodSync for F
where
	F: Fn(Params) -> X,
	X: WrapFuture<Value, Error>,
{
	fn call(&self, params: Params) -> BoxFuture<crate::Result<Value>> {
		self(params).into_future()
	}
}

impl<F: Send + Sync + 'static> RpcNotificationSimple for F
where
	F: Fn(Params),
{
	fn execute(&self, params: Params) {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static, T> RpcMethod<T> for F
where
	T: Metadata,
	F: Fn(Params, T) -> X,
	X: Future<Output = Result<Value, Error>>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<crate::Result<Value>> {
		Box::pin(self(params, meta))
	}
}

impl<F: Send + Sync + 'static, T> RpcNotification<T> for F
where
	T: Metadata,
	F: Fn(Params, T),
{
	fn execute(&self, params: Params, meta: T) {
		self(params, meta)
	}
}
