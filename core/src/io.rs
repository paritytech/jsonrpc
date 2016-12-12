use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use serde_json;
use futures::{self, Future, BoxFuture};

use calls::{RemoteProcedure, Metadata, RpcMethodSync, RpcMethodSimple, RpcMethod, RpcNotificationSimple, RpcNotification};
use types::{Params, Error, ErrorCode};
use types::{Request, Response, Call, Output};

fn read_request(request_str: &str) -> Result<Request, Error> {
	serde_json::from_str(request_str).map_err(|_| Error::new(ErrorCode::ParseError))
}

fn write_response(response: Response) -> String {
	// this should never fail
	serde_json::to_string(&response).unwrap()
}

/// Request handler
pub struct MetaIoHandler<T: Metadata> {
	methods: HashMap<String, RemoteProcedure<T>>,
}

// Don't derive, cause we don't require T to be Default
impl<T: Metadata> Default for MetaIoHandler<T> {
	fn default() -> Self {
		MetaIoHandler {
			methods: HashMap::new(),
		}
	}
}

impl<T: Metadata> MetaIoHandler<T> {
	/// Adds new supported synchronous method
	pub fn add_method<F>(&mut self, name: &str, method: F) where
		F: RpcMethodSync,
	{
		self.add_method_with_meta(name, move |params, _meta| {
			futures::done(method.call(params)).boxed()
		})
	}

	/// Adds new supported asynchronous method
	pub fn add_async_method<F>(&mut self, name: &str, method: F) where
		F: RpcMethodSimple,
	{
		self.add_method_with_meta(name, move |params, _meta| method.call(params))
	}

	/// Adds new supported notification
	pub fn add_notification<F>(&mut self, name: &str, notification: F) where
		F: RpcNotificationSimple,
	{
		self.add_notification_with_meta(name, move |params, _meta| notification.execute(params))
	}

	/// Adds new supported asynchronous method with metadata support.
	pub fn add_method_with_meta<F>(&mut self, name: &str, method: F) where
		F: RpcMethod<T>,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Method(Box::new(method)),
		);
	}

	/// Adds new supported notification with metadata support.
	pub fn add_notification_with_meta<F>(&mut self, name: &str, notification: F) where
		F: RpcNotification<T>,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Notification(Box::new(notification)),
		);
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync(&self, request: &str, meta: T) -> Option<String> {
		self.handle_request(request, meta).wait().expect("Handler calls can never fail.")
	}

	/// Handle given request asynchronously.
	pub fn handle_request(&self, request: &str, meta: T) -> BoxFuture<Option<String>, ()> {
		trace!(target: "rpc", "Request: {}.", request);
		let request = read_request(request);
		let result = match request {
			Err(error) => futures::finished(Some(Response::from(error))).boxed(),
			Ok(request) => match request {
				Request::Single(call) => {
					self.handle_call(call, meta)
						.map(|output| output.map(Response::Single))
						.boxed()
				},
				Request::Batch(calls) => {
					let futures: Vec<_> = calls.into_iter().map(move |call| self.handle_call(call, meta.clone())).collect();
					futures::future::join_all(futures).map(|outs| {
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
		};

		result.map(|response| {
			let res = response.map(write_response);
			debug!(target: "rpc", "Response: {:?}.", res);
			res
		}).boxed()
	}

	fn handle_call(&self, call: Call, meta: T) -> BoxFuture<Option<Output>, ()> {
		match call {
			Call::MethodCall(method) => {
				let params = method.params.unwrap_or(Params::None);
				let id = method.id;
				let jsonrpc = method.jsonrpc;

				let result = match self.methods.get(&method.method) {
					Some(&RemoteProcedure::Method(ref method)) => method.call(params, meta),
					_ => futures::failed(Error::new(ErrorCode::MethodNotFound)).boxed(),
				};

				result
					.then(move |result| futures::finished(Some(Output::from(result, id, jsonrpc))))
					.boxed()
			},
			Call::Notification(notification) => {
				let params = notification.params.unwrap_or(Params::None);

				if let Some(&RemoteProcedure::Notification(ref notification)) = self.methods.get(&notification.method) {
					notification.execute(params, meta);
				}

				futures::finished(None).boxed()
			},
			Call::Invalid(id) => {
				futures::finished(Some(Output::invalid_request(id))).boxed()
			},
		}
	}
}

/// Simplified `IoHandler` with no `Metadata` associated with each request.
#[derive(Default)]
pub struct IoHandler(MetaIoHandler<()>);

impl IoHandler {

	/// Handle given request asynchronously.
	pub fn handle_request(&self, request: &str) -> BoxFuture<Option<String>, ()> {
		self.0.handle_request(request, ())
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync(&self, request: &str) -> Option<String> {
		self.0.handle_request_sync(request, ())
	}
}

impl Deref for IoHandler {
	type Target = MetaIoHandler<()>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for IoHandler {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl From<IoHandler> for MetaIoHandler<()> {
	fn from(io: IoHandler) -> Self {
		io.0
	}
}

#[cfg(test)]
mod tests {
	use futures::{self, Future};
	use types::{Value};
	use super::{IoHandler};

	#[test]
	fn test_io_handler() {
		let mut io = IoHandler::default();

		io.add_method("say_hello", |_| {
			Ok(Value::String("hello".to_string()))
		});

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let mut io = IoHandler::default();

		io.add_async_method("say_hello", |_| {
			futures::finished(Value::String("hello".to_string())).boxed()
		});

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_notification() {
		use std::sync::Arc;
		use std::sync::atomic;

		let mut io = IoHandler::default();

		let called = Arc::new(atomic::AtomicBool::new(false));
		let c = called.clone();
		io.add_notification("say_hello", move |_| {
			c.store(true, atomic::Ordering::SeqCst);
		});
		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23]}"#;

		assert_eq!(io.handle_request_sync(request), None);
		assert_eq!(called.load(atomic::Ordering::SeqCst), true);
	}

	#[test]
	fn test_method_not_found() {
		let mut io = IoHandler::default();

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_send_sync() {
		fn is_send_sync<T>(_obj: T) where
			T: Send + Sync
		{
			true
		}

		let io = IoHandler::default();

		assert!(is_send_sync(io))
	}
}
