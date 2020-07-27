use std::collections::{
	hash_map::{IntoIter, Iter},
	HashMap,
};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;

use futures::{self, future, Future, FutureExt};
use serde_json;

use crate::calls::{
	Metadata, RemoteProcedure, RpcMethod, RpcMethodSimple, RpcMethodSync, RpcNotification, RpcNotificationSimple,
};
use crate::middleware::{self, Middleware};
use crate::types::{Call, Output, Request, Response};
use crate::types::{Error, ErrorCode, Version};

/// A type representing middleware or RPC response before serialization.
pub type FutureResponse = Pin<Box<dyn Future<Output = Option<Response>> + Send>>;

/// A type representing middleware or RPC call output.
pub type FutureOutput = Pin<Box<dyn Future<Output = Option<Output>> + Send>>;

/// A type representing future string response.
pub type FutureResult<F, G> = future::Map<
	future::Either<future::Ready<Option<Response>>, FutureRpcResult<F, G>>,
	fn(Option<Response>) -> Option<String>,
>;

/// A type representing a result of a single method call.
pub type FutureRpcOutput<F> = future::Either<F, future::Either<FutureOutput, future::Ready<Option<Output>>>>;

/// A type representing an optional `Response` for RPC `Request`.
pub type FutureRpcResult<F, G> = future::Either<
	F,
	future::Either<
		future::Map<FutureRpcOutput<G>, fn(Option<Output>) -> Option<Response>>,
		future::Map<future::JoinAll<FutureRpcOutput<G>>, fn(Vec<Option<Output>>) -> Option<Response>>,
	>,
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
	fn is_version_valid(self, version: Option<Version>) -> bool {
		match (self, version) {
			(Compatibility::V1, None) | (Compatibility::V2, Some(Version::V2)) | (Compatibility::Both, _) => true,
			_ => false,
		}
	}

	fn default_version(self) -> Option<Version> {
		match self {
			Compatibility::V1 => None,
			Compatibility::V2 | Compatibility::Both => Some(Version::V2),
		}
	}
}

/// Request handler
///
/// By default compatible only with jsonrpc v2
#[derive(Clone, Debug)]
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

impl<T: Metadata, S: Middleware<T>> IntoIterator for MetaIoHandler<T, S> {
	type Item = (String, RemoteProcedure<T>);
	type IntoIter = IntoIter<String, RemoteProcedure<T>>;

	fn into_iter(self) -> Self::IntoIter {
		self.methods.into_iter()
	}
}

impl<'a, T: Metadata, S: Middleware<T>> IntoIterator for &'a MetaIoHandler<T, S> {
	type Item = (&'a String, &'a RemoteProcedure<T>);
	type IntoIter = Iter<'a, String, RemoteProcedure<T>>;

	fn into_iter(self) -> Self::IntoIter {
		self.methods.iter()
	}
}

impl<T: Metadata> MetaIoHandler<T> {
	/// Creates new `MetaIoHandler` compatible with specified protocol version.
	pub fn with_compatibility(compatibility: Compatibility) -> Self {
		MetaIoHandler {
			compatibility,
			middleware: Default::default(),
			methods: Default::default(),
		}
	}
}

impl<T: Metadata, S: Middleware<T>> MetaIoHandler<T, S> {
	/// Creates new `MetaIoHandler`
	pub fn new(compatibility: Compatibility, middleware: S) -> Self {
		MetaIoHandler {
			compatibility,
			middleware,
			methods: Default::default(),
		}
	}

	/// Creates new `MetaIoHandler` with specified middleware.
	pub fn with_middleware(middleware: S) -> Self {
		MetaIoHandler {
			compatibility: Default::default(),
			middleware,
			methods: Default::default(),
		}
	}

	/// Adds an alias to a method.
	pub fn add_alias(&mut self, alias: &str, other: &str) {
		self.methods.insert(alias.into(), RemoteProcedure::Alias(other.into()));
	}

	/// Adds new supported synchronous method.
	///
	/// A backward-compatible wrapper.
	pub fn add_sync_method<F>(&mut self, name: &str, method: F)
	where
		F: RpcMethodSync,
	{
		self.add_method(name, move |params| method.call(params))
	}

	/// Adds new supported asynchronous method.
	pub fn add_method<F>(&mut self, name: &str, method: F)
	where
		F: RpcMethodSimple,
	{
		self.add_method_with_meta(name, move |params, _meta| method.call(params))
	}

	/// Adds new supported notification
	pub fn add_notification<F>(&mut self, name: &str, notification: F)
	where
		F: RpcNotificationSimple,
	{
		self.add_notification_with_meta(name, move |params, _meta| notification.execute(params))
	}

	/// Adds new supported asynchronous method with metadata support.
	pub fn add_method_with_meta<F>(&mut self, name: &str, method: F)
	where
		F: RpcMethod<T>,
	{
		self.methods
			.insert(name.into(), RemoteProcedure::Method(Arc::new(method)));
	}

	/// Adds new supported notification with metadata support.
	pub fn add_notification_with_meta<F>(&mut self, name: &str, notification: F)
	where
		F: RpcNotification<T>,
	{
		self.methods
			.insert(name.into(), RemoteProcedure::Notification(Arc::new(notification)));
	}

	/// Extend this `MetaIoHandler` with methods defined elsewhere.
	pub fn extend_with<F>(&mut self, methods: F)
	where
		F: IntoIterator<Item = (String, RemoteProcedure<T>)>,
	{
		self.methods.extend(methods)
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync(&self, request: &str, meta: T) -> Option<String> {
		futures::executor::block_on(self.handle_request(request, meta))
	}

	/// Handle given request asynchronously.
	pub fn handle_request(&self, request: &str, meta: T) -> FutureResult<S::Future, S::CallFuture> {
		use self::future::Either::{Left, Right};
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
			Err(error) => Left(future::ready(Some(Response::from(
				error,
				self.compatibility.default_version(),
			)))),
			Ok(request) => Right(self.handle_rpc_request(request, meta)),
		};

		result.map(as_string)
	}

	/// Handle deserialized RPC request.
	pub fn handle_rpc_request(&self, request: Request, meta: T) -> FutureRpcResult<S::Future, S::CallFuture> {
		use self::future::Either::{Left, Right};

		fn output_as_response(output: Option<Output>) -> Option<Response> {
			output.map(Response::Single)
		}

		fn outputs_as_batch(outs: Vec<Option<Output>>) -> Option<Response> {
			let outs: Vec<_> = outs.into_iter().filter_map(|v| v).collect();
			if outs.is_empty() {
				None
			} else {
				Some(Response::Batch(outs))
			}
		}

		self.middleware
			.on_request(request, meta, |request, meta| match request {
				Request::Single(call) => Left(
					self.handle_call(call, meta)
						.map(output_as_response as fn(Option<Output>) -> Option<Response>),
				),
				Request::Batch(calls) => {
					let futures: Vec<_> = calls
						.into_iter()
						.map(move |call| self.handle_call(call, meta.clone()))
						.collect();
					Right(
						future::join_all(futures).map(outputs_as_batch as fn(Vec<Option<Output>>) -> Option<Response>),
					)
				}
			})
	}

	/// Handle single call asynchronously.
	pub fn handle_call(&self, call: Call, meta: T) -> FutureRpcOutput<S::CallFuture> {
		use self::future::Either::{Left, Right};

		self.middleware.on_call(call, meta, |call, meta| match call {
			Call::MethodCall(method) => {
				let params = method.params;
				let id = method.id;
				let jsonrpc = method.jsonrpc;
				let valid_version = self.compatibility.is_version_valid(jsonrpc);

				let call_method = |method: &Arc<dyn RpcMethod<T>>| method.call(params, meta);

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
					Ok(result) => Left(Box::pin(
						result.then(move |result| future::ready(Some(Output::from(result, id, jsonrpc)))),
					) as _),
					Err(err) => Right(future::ready(Some(Output::from(Err(err), id, jsonrpc)))),
				}
			}
			Call::Notification(notification) => {
				let params = notification.params;
				let jsonrpc = notification.jsonrpc;
				if !self.compatibility.is_version_valid(jsonrpc) {
					return Right(future::ready(None));
				}

				match self.methods.get(&notification.method) {
					Some(&RemoteProcedure::Notification(ref notification)) => {
						notification.execute(params, meta);
					}
					Some(&RemoteProcedure::Alias(ref alias)) => {
						if let Some(&RemoteProcedure::Notification(ref notification)) = self.methods.get(alias) {
							notification.execute(params, meta);
						}
					}
					_ => {}
				}

				Right(future::ready(None))
			}
			Call::Invalid { id } => Right(future::ready(Some(Output::invalid_request(
				id,
				self.compatibility.default_version(),
			)))),
		})
	}

	/// Returns an iterator visiting all methods in arbitrary order.
	pub fn iter(&self) -> impl Iterator<Item = (&String, &RemoteProcedure<T>)> {
		self.methods.iter()
	}
}

/// A type that can augment `MetaIoHandler`.
///
/// This allows your code to accept generic extensions for `IoHandler`
/// and compose them to create the RPC server.
pub trait IoHandlerExtension<M: Metadata = ()> {
	/// Extend given `handler` with additional methods.
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>);
}

macro_rules! impl_io_handler_extension {
	($( $x:ident, )*) => {
		impl<M, $( $x, )*> IoHandlerExtension<M> for ($( $x, )*) where
			M: Metadata,
			$(
				$x: IoHandlerExtension<M>,
			)*
			{
				#[allow(unused)]
				fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
					#[allow(non_snake_case)]
					let (
						$( $x, )*
					) = self;
					$(
						$x.augment(handler);
					)*
				}
			}
	}
}

impl_io_handler_extension!();
impl_io_handler_extension!(A,);
impl_io_handler_extension!(A, B,);
impl_io_handler_extension!(A, B, C,);
impl_io_handler_extension!(A, B, C, D,);
impl_io_handler_extension!(A, B, C, D, E,);
impl_io_handler_extension!(A, B, C, D, E, F,);
impl_io_handler_extension!(A, B, C, D, E, F, G,);
impl_io_handler_extension!(A, B, C, D, E, F, G, H,);
impl_io_handler_extension!(A, B, C, D, E, F, G, H, I,);
impl_io_handler_extension!(A, B, C, D, E, F, G, H, I, J,);
impl_io_handler_extension!(A, B, C, D, E, F, G, H, I, J, K,);
impl_io_handler_extension!(A, B, C, D, E, F, G, H, I, J, K, L,);

impl<M: Metadata> IoHandlerExtension<M> for Vec<(String, RemoteProcedure<M>)> {
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
		handler.methods.extend(self)
	}
}

impl<M: Metadata> IoHandlerExtension<M> for HashMap<String, RemoteProcedure<M>> {
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
		handler.methods.extend(self)
	}
}

impl<M: Metadata, S2: Middleware<M>> IoHandlerExtension<M> for MetaIoHandler<M, S2> {
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
		handler.methods.extend(self.methods)
	}
}

impl<M: Metadata, T: IoHandlerExtension<M>> IoHandlerExtension<M> for Option<T> {
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
		if let Some(x) = self {
			x.augment(handler)
		}
	}
}

/// Simplified `IoHandler` with no `Metadata` associated with each request.
#[derive(Clone, Debug, Default)]
pub struct IoHandler<M: Metadata = ()>(MetaIoHandler<M>);

impl<T: Metadata> IntoIterator for IoHandler<T> {
	type Item = <MetaIoHandler<T> as IntoIterator>::Item;
	type IntoIter = <MetaIoHandler<T> as IntoIterator>::IntoIter;

	fn into_iter(self) -> Self::IntoIter {
		self.0.into_iter()
	}
}

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
	pub fn handle_request(&self, request: &str) -> FutureResult<FutureResponse, FutureOutput> {
		self.0.handle_request(request, M::default())
	}

	/// Handle deserialized RPC request asynchronously.
	pub fn handle_rpc_request(&self, request: Request) -> FutureRpcResult<FutureResponse, FutureOutput> {
		self.0.handle_rpc_request(request, M::default())
	}

	/// Handle single Call asynchronously.
	pub fn handle_call(&self, call: Call) -> FutureRpcOutput<FutureOutput> {
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

impl<M: Metadata> IoHandlerExtension<M> for IoHandler<M> {
	fn augment<S: Middleware<M>>(self, handler: &mut MetaIoHandler<M, S>) {
		handler.methods.extend(self.0.methods)
	}
}

fn read_request(request_str: &str) -> Result<Request, Error> {
	crate::serde_from_str(request_str).map_err(|_| Error::new(ErrorCode::ParseError))
}

fn write_response(response: Response) -> String {
	// this should never fail
	serde_json::to_string(&response).unwrap()
}

#[cfg(test)]
mod tests {
	use super::{Compatibility, IoHandler};
	use crate::types::Value;
	use futures::future;

	#[test]
	fn test_io_handler() {
		let mut io = IoHandler::new();

		io.add_method("say_hello", |_| async { Ok(Value::String("hello".to_string())) });

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_io_handler_1dot0() {
		let mut io = IoHandler::with_compatibility(Compatibility::Both);

		io.add_method("say_hello", |_| async { Ok(Value::String("hello".to_string())) });

		let request = r#"{"method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let mut io = IoHandler::new();

		io.add_method("say_hello", |_| future::ready(Ok(Value::String("hello".to_string()))));

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_notification() {
		use std::sync::atomic;
		use std::sync::Arc;

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
		io.add_method("say_hello", |_| async { Ok(Value::String("hello".to_string())) });
		io.add_alias("say_hello_alias", "say_hello");

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello_alias", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_notification_alias() {
		use std::sync::atomic;
		use std::sync::Arc;

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
	fn test_batch_notification() {
		use std::sync::atomic;
		use std::sync::Arc;

		let mut io = IoHandler::new();

		let called = Arc::new(atomic::AtomicBool::new(false));
		let c = called.clone();
		io.add_notification("say_hello", move |_| {
			c.store(true, atomic::Ordering::SeqCst);
		});

		let request = r#"[{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23]}]"#;
		assert_eq!(io.handle_request_sync(request), None);
		assert_eq!(called.load(atomic::Ordering::SeqCst), true);
	}

	#[test]
	fn test_send_sync() {
		fn is_send_sync<T>(_obj: T) -> bool
		where
			T: Send + Sync,
		{
			true
		}

		let io = IoHandler::new();

		assert!(is_send_sync(io))
	}

	#[test]
	fn test_extending_by_multiple_delegates() {
		use super::IoHandlerExtension;
		use crate::delegates::IoDelegate;
		use std::sync::Arc;

		struct Test;
		impl Test {
			fn abc(&self, _p: crate::Params) -> crate::BoxFuture<crate::Result<Value>> {
				Box::pin(async { Ok(5.into()) })
			}
		}

		let mut io = IoHandler::new();
		let mut del1 = IoDelegate::new(Arc::new(Test));
		del1.add_method("rpc_test", Test::abc);
		let mut del2 = IoDelegate::new(Arc::new(Test));
		del2.add_method("rpc_test", Test::abc);

		fn augment<X: IoHandlerExtension>(x: X, io: &mut IoHandler) {
			x.augment(io);
		}

		augment((del1, del2), &mut io);
	}
}
