use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use serde_json;
use futures::{self, Future, BoxFuture};

use calls::{RemoteProcedure, Metadata, RpcMethodSync, RpcMethodSimple, RpcMethod, RpcNotificationSimple, RpcNotification};
use middleware::{self, Middleware};
use types::{Params, Error, ErrorCode, Version};
use types::{Request, Response, Call, Output};

/// Type representing middleware or RPC response before serialization.
pub type FutureResponse = BoxFuture<Option<Response>, ()>;

/// `IoHandler` json-rpc protocol compatibility
#[derive(Clone, Copy)]
pub enum Compatibility {
	/// Compatible only with JSON-RPC 1.x
	V1,
	/// Compatible only with JSON-RPC 2.0
	V2,
	/// Compatible with both
	Both,
}

impl Default for Compatibility {
	fn default() -> Self {
		Compatibility::V2
	}
}

impl Compatibility {
	fn is_version_valid(&self, version: Option<Version>) -> bool {
		match (*self, version) {
			(Compatibility::V1, None) |
			(Compatibility::V2, Some(Version::V2)) |
			(Compatibility::Both, _) => true,
			_ => false,
		}
	}

	fn default_version(&self) -> Option<Version> {
		match *self {
			Compatibility::V1 => None,
			Compatibility::V2 | Compatibility::Both => Some(Version::V2),
		}
	}
}

/// Request handler
///
/// By default compatible only with jsonrpc v2
pub struct MetaIoHandler<T: Metadata, S: Middleware<T> = middleware::Noop> {
	middleware: S,
	compatibility: Compatibility,
	methods: HashMap<String, RemoteProcedure<T>>,
}

impl<T: Metadata> Default for MetaIoHandler<T> {
	fn default() -> Self {
		MetaIoHandler::with_compatibility(Default::default())
	}
}

impl<T: Metadata> MetaIoHandler<T> {
	/// Creates new `MetaIoHandler` compatible with specified protocol version.
	pub fn with_compatibility(compatibility: Compatibility) -> Self {
		MetaIoHandler {
			compatibility: compatibility,
			middleware: Default::default(),
			methods: Default::default(),
		}
	}
}


impl<T: Metadata, S: Middleware<T>> MetaIoHandler<T, S> {
	/// Creates new `MetaIoHandler`
	pub fn new(compatibility: Compatibility, middleware: S) -> Self {
		MetaIoHandler {
			compatibility: compatibility,
			middleware: middleware,
			methods: Default::default(),
		}
	}

	/// Creates new `MetaIoHandler` with specified middleware.
	pub fn with_middleware(middleware: S) -> Self {
		MetaIoHandler {
			compatibility: Default::default(),
			middleware: middleware,
			methods: Default::default(),
		}
	}

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

	/// Extend this `MetaIoHandler` with methods defined elsewhere.
	pub fn extend_with<F>(&mut self, methods: F) where
		F: Into<HashMap<String, RemoteProcedure<T>>>
	{
		self.methods.extend(methods.into())
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
			Err(error) => futures::finished(Some(Response::from(error, self.compatibility.default_version()))).boxed(),
			Ok(request) => self.middleware.on_request(request, meta, |request, meta| match request {
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
			}),
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
				let valid_version = self.compatibility.is_version_valid(jsonrpc);

				let result = match (valid_version, self.methods.get(&method.method)) {
					(false, _) => futures::failed(Error::invalid_version()).boxed(),
					(true, Some(&RemoteProcedure::Method(ref method))) => method.call(params, meta),
					(true, _) => futures::failed(Error::method_not_found()).boxed(),
				};

				result
					.then(move |result| futures::finished(Some(Output::from(result, id, jsonrpc))))
					.boxed()
			},
			Call::Notification(notification) => {
				let params = notification.params.unwrap_or(Params::None);
				let jsonrpc = notification.jsonrpc;
				if !self.compatibility.is_version_valid(jsonrpc) {
					return futures::finished(None).boxed();
				}

				if let Some(&RemoteProcedure::Notification(ref notification)) = self.methods.get(&notification.method) {
					notification.execute(params, meta);
				}

				futures::finished(None).boxed()
			},
			Call::Invalid(id) => {
				futures::finished(Some(Output::invalid_request(id, self.compatibility.default_version()))).boxed()
			},
		}
	}
}

/// Simplified `IoHandler` with no `Metadata` associated with each request.
#[derive(Default)]
pub struct IoHandler<M: Metadata = ()>(MetaIoHandler<M>);

// Type inference helper
impl IoHandler {
	/// Creates new `IoHandler` without any metadata.
	pub fn new() -> Self {
		IoHandler::default()
	}

	/// Creates new `IoHandler` without any metadata compatible with specified protocol version.
	pub fn with_compatibility(compatibility: Compatibility) -> Self {
		IoHandler(MetaIoHandler::with_compatibility(compatibility))
	}
}

impl<M: Metadata> IoHandler<M> {

	/// Handle given request asynchronously.
	pub fn handle_request(&self, request: &str) -> BoxFuture<Option<String>, ()> {
		self.0.handle_request(request, M::default())
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync(&self, request: &str) -> Option<String> {
		self.0.handle_request_sync(request, M::default())
	}
}

impl<M: Metadata> Deref for IoHandler<M> {
	type Target = MetaIoHandler<M>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<M: Metadata> DerefMut for IoHandler<M> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl From<IoHandler> for MetaIoHandler<()> {
	fn from(io: IoHandler) -> Self {
		io.0
	}
}

fn read_request(request_str: &str) -> Result<Request, Error> {
	serde_json::from_str(request_str).map_err(|_| Error::new(ErrorCode::ParseError))
}

fn write_response(response: Response) -> String {
	// this should never fail
	serde_json::to_string(&response).unwrap()
}

#[cfg(test)]
mod tests {
	use futures::{self, Future};
	use types::{Value};
	use super::{IoHandler};

	#[test]
	fn test_io_handler() {
		let mut io = IoHandler::new();

		io.add_method("say_hello", |_| {
			Ok(Value::String("hello".to_string()))
		});

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let mut io = IoHandler::new();

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

		let mut io = IoHandler::new();

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
		let io = IoHandler::new();

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found","data":null},"id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_send_sync() {
		fn is_send_sync<T>(_obj: T) -> bool where
			T: Send + Sync
		{
			true
		}

		let io = IoHandler::new();

		assert!(is_send_sync(io))
	}
}
