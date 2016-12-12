
use types::{Params, Value, Error};
use futures::BoxFuture;

pub trait Metadata: Clone + 'static {}
impl Metadata for () {}

pub trait RpcMethodSync: 'static {
	fn call(&self, params: Params) -> Result<Value, Error>;
}

impl<F: 'static> RpcMethodSync for F where
	F: Fn(Params) -> Result<Value, Error>,
{
	fn call(&self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

pub trait RpcMethodSimple: 'static {
	fn call(&self, params: Params) -> BoxFuture<Value, Error>;
}

impl<F: 'static> RpcMethodSimple for F where
	F: Fn(Params) -> BoxFuture<Value, Error>,
{
	fn call(&self, params: Params) -> BoxFuture<Value, Error> {
		self(params)
	}
}

pub trait RpcNotificationSimple: 'static {
	fn execute(&self, params: Params);
}

impl<F: 'static> RpcNotificationSimple for F where
	F: Fn(Params),
{
	fn execute(&self, params: Params) {
		self(params)
	}
}

pub trait RpcMethod<T: Metadata>: 'static {
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error>;
}

impl<F: 'static, T> RpcMethod<T> for F where
	T: Metadata,
	F: Fn(Params, T) -> BoxFuture<Value, Error>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error> {
		self(params, meta)
	}
}

pub trait RpcNotification<T: Metadata>: 'static {
	fn execute(&self, params: Params, meta: T);
}

impl<F: 'static, T> RpcNotification<T> for F where
	T: Metadata,
	F: Fn(Params, T),
{
	fn execute(&self, params: Params, meta: T) {
		self(params, meta)
	}
}

