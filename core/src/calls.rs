
use types::{Params, Value, Error};
use futures::BoxFuture;

/// Metadata marker trait
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

pub enum RemoteProcedure<T: Metadata> {
	Method(Box<RpcMethod<T>>),
	Notification(Box<RpcNotification<T>>)
}

impl<F: Send + Sync + 'static> RpcMethodSync for F where
	F: Fn(Params) -> Result<Value, Error>,
{
	fn call(&self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

impl<F: Send + Sync + 'static> RpcMethodSimple for F where
	F: Fn(Params) -> BoxFuture<Value, Error>,
{
	fn call(&self, params: Params) -> BoxFuture<Value, Error> {
		self(params)
	}
}

impl<F: Send + Sync + 'static> RpcNotificationSimple for F where
	F: Fn(Params),
{
	fn execute(&self, params: Params) {
		self(params)
	}
}

impl<F: Send + Sync + 'static, T> RpcMethod<T> for F where
	T: Metadata,
	F: Fn(Params, T) -> BoxFuture<Value, Error>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error> {
		self(params, meta)
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
