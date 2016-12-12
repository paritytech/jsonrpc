use futures::{self, Future, BoxFuture};

use calls::{Metadata, RpcMethod, RpcNotification};
use types::{Params, Error, ErrorCode, Version};
use types::{Request, Response, Output, Call};

pub struct RpcRequest<T: Metadata> {
	call: RpcCall<T>,
}

impl<T: Metadata> RpcRequest<T> {
	pub fn execute(&'static self, request: Request, meta: T) -> BoxFuture<Option<Response>, ()> {
		match request {
			Request::Single(call) => {
				self.call.execute(call, meta)
					.map(|output| output.map(Response::Single))
					.boxed()
			},
			Request::Batch(calls) => {
				futures::future::join_all(
					calls.into_iter().map(move |call| self.call.execute(call, meta.clone()))
				).map(|outs| {
					let outs: Vec<_> = outs.into_iter().filter_map(|v| v).collect();
					if outs.is_empty() {
						None
					} else {
						Some(Response::Batch(outs))
					}
				})
				.boxed()
			},
		}
	}
}

impl<T: Metadata> From<RpcCall<T>> for RpcRequest<T> {
	fn from(call: RpcCall<T>) -> Self {
		RpcRequest {
			call: call,
		}
	}
}

pub struct RpcCall<T: Metadata> {
	method: Box<Fn(Call, T) -> BoxFuture<Option<Output>, ()>>,
}

impl<T: Metadata> RpcCall<T> {
	pub fn execute(&self, call: Call, meta: T) -> BoxFuture<Option<Output>, ()> {
		let method = &self.method;
		method(call, meta)
	}

	fn new<F>(func: F) -> Self where
		F: Fn(Call, T) -> BoxFuture<Option<Output>, ()> + 'static,
	{
		RpcCall {
			method: Box::new(func),
		}
	}

	pub fn from_notification<F>(rpc_notification: F) -> Self where
		F: RpcNotification<T>,
	{
		Self::new(move |call, meta| match call {
			Call::MethodCall(method) => {
				// TODO [ToDr] Return method not found?
				unimplemented!()
			},
			Call::Notification(notification) => {
				let params = notification.params.unwrap_or(Params::None);
				rpc_notification.execute(params, meta);
				futures::finished(None).boxed()
			},
			Call::Invalid(id) => futures::finished(Some(Output::invalid_request(id))).boxed(),
		})
	}

	pub fn from_method<F>(rpc_method: F) -> Self where
		F: RpcMethod<T>,
	{
		Self::new(move |call, meta| match call {
			Call::MethodCall(method) => {
				let params = method.params.unwrap_or(Params::None);
				let id = method.id;
				let jsonrpc = method.jsonrpc;

				rpc_method.call(params, meta)
					.then(move |result| futures::finished(Some(Output::from(result, id, jsonrpc))))
					.boxed()
			},
			Call::Notification(notification) => {
				let params = notification.params.unwrap_or(Params::None);

				rpc_method.call(params, meta)
					.then(|_| futures::finished(None))
					.boxed()
			},
			Call::Invalid(id) => futures::finished(Some(Output::invalid_request(id))).boxed(),
		})
	}
}
