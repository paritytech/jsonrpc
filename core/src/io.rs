use std::sync::Arc;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use serde_json;
use futures::{self, future, Future};

use calls::{RemoteProcedure, Metadata, RpcMethodSimple, RpcMethod, RpcNotificationSimple, RpcNotification};
use middleware::{self, Middleware};
use types::{Params, Error, ErrorCode, Version};
use types::{Request, Response, Call, Output};

/// A type representing middleware or RPC response before serialization.
pub type FutureResponse = Box<Future<Item=Option<Response>, Error=()> + Send>;

/// A type representing future string response.
pub type FutureResult<F> = future::Map<
	future::Either<future::FutureResult<Option<Response>, ()>, F>,
	fn(Option<Response>) -> Option<String>,
>;

/// A type representing a result of a single method call.
pub type FutureOutput = future::Either<
	Box<Future<Item=Option<Output>, Error=()> + Send>,
	future::FutureResult<Option<Output>, ()>,
>;

/// `IoHandler` json-rpc protocol compatibility
#[derive(Debug, Clone, Copy)]
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
#[derive(Debug)]
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

	/// Adds an alias to a method.
	pub fn add_alias(&mut self, alias: &str, other: &str) {
		self.methods.insert(
			alias.into(),
			RemoteProcedure::Alias(other.into()),
		);
	}

	/// Adds new supported asynchronous method
	pub fn add_method<F>(&mut self, name: &str, method: F) where
		F: RpcMethodSimple,
	{
		self.add_method_with_meta(name, move |params, _meta| {
			method.call(params)
		})
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
			RemoteProcedure::Method(Arc::new(method)),
		);
	}

	/// Adds new supported notification with metadata support.
	pub fn add_notification_with_meta<F>(&mut self, name: &str, notification: F) where
		F: RpcNotification<T>,
	{
		self.methods.insert(
			name.into(),
			RemoteProcedure::Notification(Arc::new(notification)),
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
	pub fn handle_request(&self, request: &str, meta: T) -> FutureResult<S::Future> {
		use self::future::Either::{A, B};
		fn as_string(response: Option<Response>) -> Option<String> {
			let res = response.map(write_response);
			debug!(target: "rpc", "Response: {}.", match res {
				Some(ref res) => res,
				None => "None",
			});
			res
		}

		trace!(target: "rpc", "Request: {}.", request);
		let request = read_request(request);
		let result = match request {
			Err(error) => A(futures::finished(Some(Response::from(error, self.compatibility.default_version())))),
			Ok(request) => B(self.handle_rpc_request(request, meta)),
		};

		result.map(as_string)
	}

	/// Handle deserialized RPC request.
	pub fn handle_rpc_request(&self, request: Request, meta: T) -> S::Future {
		use self::future::Either::{A, B};

		self.middleware.on_request(request, meta, |request, meta| match request {
			Request::Single(call) => {
				A(self.handle_call(call, meta).map(|output| output.map(Response::Single)))
			},
			Request::Batch(calls) => {
				let futures: Vec<_> = calls.into_iter().map(move |call| self.handle_call(call, meta.clone())).collect();
				B(futures::future::join_all(futures).map(|outs| {
					let outs: Vec<_> = outs.into_iter().filter_map(|v| v).collect();
					if outs.is_empty() {
						None
					} else {
						Some(Response::Batch(outs))
					}
				}))
			},
		})
	}

	/// Handle single call asynchronously.
	pub fn handle_call(&self, call: Call, meta: T) -> FutureOutput {
		use self::future::Either::{A, B};

		match call {
			Call::MethodCall(method) => {
				let params = method.params.unwrap_or(Params::None);
				let id = method.id;
				let jsonrpc = method.jsonrpc;
				let valid_version = self.compatibility.is_version_valid(jsonrpc);

				let call_method = |method: &Arc<RpcMethod<T>>| {
					let method = method.clone();
					futures::lazy(move || method.call(params, meta))
				};

				let result = match (valid_version, self.methods.get(&method.method)) {
					(false, _) => Err(Error::invalid_version()),
					(true, Some(&RemoteProcedure::Method(ref method))) => Ok(call_method(method)),
					(true, Some(&RemoteProcedure::Alias(ref alias))) => match self.methods.get(alias) {
						Some(&RemoteProcedure::Method(ref method)) => Ok(call_method(method)),
						_ => Err(Error::method_not_found()),
					},
					(true, _) => Err(Error::method_not_found()),
				};

				match result {
					Ok(result) => A(Box::new(
						result.then(move |result| futures::finished(Some(Output::from(result, id, jsonrpc))))
					)),
					Err(err) => B(futures::finished(Some(Output::from(Err(err), id, jsonrpc)))),
				}
			},
			Call::Notification(notification) => {
				let params = notification.params.unwrap_or(Params::None);
				let jsonrpc = notification.jsonrpc;
				if !self.compatibility.is_version_valid(jsonrpc) {
					return B(futures::finished(None));
				}

				match self.methods.get(&notification.method) {
					Some(&RemoteProcedure::Notification(ref notification)) => {
						notification.execute(params, meta);
					},
					Some(&RemoteProcedure::Alias(ref alias)) => {
						if let Some(&RemoteProcedure::Notification(ref notification)) = self.methods.get(alias) {
							notification.execute(params, meta);
						}
					},
					_ => {},
				}

				B(futures::finished(None))
			},
			Call::Invalid(id) => {
				B(futures::finished(Some(Output::invalid_request(id, self.compatibility.default_version()))))
			},
		}
	}
}

/// Simplified `IoHandler` with no `Metadata` associated with each request.
#[derive(Debug, Default)]
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

impl<M: Metadata + Default> IoHandler<M> {
	/// Handle given string request asynchronously.
	pub fn handle_request(&self, request: &str) -> FutureResult<FutureResponse> {
		self.0.handle_request(request, M::default())
	}

	/// Handle deserialized RPC request asynchronously.
	pub fn handle_rpc_request(&self, request: Request) -> FutureResponse {
		self.0.handle_rpc_request(request, M::default())
	}

	/// Handle single Call asynchronously.
	pub fn handle_call(&self, call: Call) -> FutureOutput {
		self.0.handle_call(call, M::default())
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
	use futures;
	use types::{Value};
	use super::{IoHandler, Compatibility};

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
	fn test_io_handler_1dot0() {
		let mut io = IoHandler::with_compatibility(Compatibility::Both);

		io.add_method("say_hello", |_| {
			Ok(Value::String("hello".to_string()))
		});

		let request = r#"{"method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let mut io = IoHandler::new();

		io.add_method("say_hello", |_| {
			futures::finished(Value::String("hello".to_string()))
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
		let response = r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_method_alias() {
		let mut io = IoHandler::new();
		io.add_method("say_hello", |_| {
			Ok(Value::String("hello".to_string()))
		});
		io.add_alias("say_hello_alias", "say_hello");


		let request = r#"{"jsonrpc": "2.0", "method": "say_hello_alias", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_notification_alias() {
		use std::sync::Arc;
		use std::sync::atomic;

		let mut io = IoHandler::new();

		let called = Arc::new(atomic::AtomicBool::new(false));
		let c = called.clone();
		io.add_notification("say_hello", move |_| {
			c.store(true, atomic::Ordering::SeqCst);
		});
		io.add_alias("say_hello_alias", "say_hello");

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello_alias", "params": [42, 23]}"#;
		assert_eq!(io.handle_request_sync(request), None);
		assert_eq!(called.load(atomic::Ordering::SeqCst), true);
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
