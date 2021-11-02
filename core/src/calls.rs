use crate::types::{Error, Id, Params, SerializedOutput, Value, Version};
use crate::BoxFuture;
use futures_util::{self, future, FutureExt};
use serde::Serialize;
use std::fmt;
use std::future::Future;
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
		Box::pin(async { self })
	}
}

impl<T, E> WrapFuture<T, E> for BoxFuture<Result<T, E>> {
	fn into_future(self) -> BoxFuture<Result<T, E>> {
		self
	}
}

/// A synchronous or asynchronous method.
pub trait RpcMethodSync<R = Value>: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params) -> BoxFuture<crate::Result<R>>;
}

/// Asynchronous Method
pub trait RpcMethodSimple<R = Value>: Send + Sync + 'static {
	/// Output future
	type Out: Future<Output = Result<R, Error>> + Send;
	/// Call method
	fn call(&self, params: Params) -> Self::Out;
}

/// Asynchronous Method with Metadata
pub trait RpcMethod<T: Metadata, R = Value>: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params, meta: T) -> BoxFuture<crate::Result<R>>;
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

/// Asynchronous Method with Metadata, returning a serialized JSONRPC response
pub trait RpcMethodWithSerializedOutput<T: Metadata>: Send + Sync + 'static {
	fn call(&self, params: Params, meta: T, jsonrpc: Option<Version>, id: Id) -> BoxFuture<Option<SerializedOutput>>;
}

/// Wraps an RpcMethod into an RpcMethodWithSerializedOutput
pub(crate) fn rpc_wrap<T: Metadata, R: Serialize + Send + 'static, F: RpcMethod<T, R>>(
	f: F,
) -> Arc<dyn RpcMethodWithSerializedOutput<T>> {
	Arc::new(move |params: Params, meta: T, jsonrpc: Option<Version>, id: Id| {
		let result = f.call(params, meta);
		result.then(move |r| future::ready(Some(SerializedOutput::from(r, id, jsonrpc))))
	})
}

/// Possible Remote Procedures with Metadata
#[derive(Clone)]
pub enum RemoteProcedure<T: Metadata> {
	/// A method call
	Method(Arc<dyn RpcMethodWithSerializedOutput<T>>),
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

impl<F: Send + Sync + 'static, X: Send + 'static, R> RpcMethodSimple<R> for F
where
	F: Fn(Params) -> X,
	X: Future<Output = Result<R, Error>>,
{
	type Out = X;
	fn call(&self, params: Params) -> Self::Out {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static, R> RpcMethodSync<R> for F
where
	F: Fn(Params) -> X,
	X: WrapFuture<R, Error>,
{
	fn call(&self, params: Params) -> BoxFuture<crate::Result<R>> {
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

impl<F: Send + Sync + 'static, X: Send + 'static, T, R> RpcMethod<T, R> for F
where
	T: Metadata,
	F: Fn(Params, T) -> X,
	X: Future<Output = Result<R, Error>>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<crate::Result<R>> {
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

impl<F: Send + Sync + 'static, X: Send + 'static, T> RpcMethodWithSerializedOutput<T> for F
where
	T: Metadata,
	F: Fn(Params, T, Option<Version>, Id) -> X,
	X: Future<Output = Option<SerializedOutput>>,
{
	fn call(&self, params: Params, meta: T, jsonrpc: Option<Version>, id: Id) -> BoxFuture<Option<SerializedOutput>> {
		Box::pin(self(params, meta, jsonrpc, id))
	}
}
