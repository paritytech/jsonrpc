//! JSON-RPC client implementation.

#![deny(missing_docs)]

use jsonrpc_core::futures::channel::{mpsc, oneshot};
use jsonrpc_core::futures::{
	self,
	task::{Context, Poll},
	Future, Stream, StreamExt,
};
use jsonrpc_core::{Error, Params};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::marker::PhantomData;
use std::pin::Pin;

pub mod transports;

#[cfg(test)]
mod logger;

/// The errors returned by the client.
#[derive(Debug, derive_more::Display)]
pub enum RpcError {
	/// An error returned by the server.
	#[display(fmt = "Server returned rpc error {}", _0)]
	JsonRpcError(Error),
	/// Failure to parse server response.
	#[display(fmt = "Failed to parse server response as {}: {}", _0, _1)]
	ParseError(String, Box<dyn std::error::Error + Send>),
	/// Request timed out.
	#[display(fmt = "Request timed out")]
	Timeout,
	/// A general client error.
	#[display(fmt = "Client error: {}", _0)]
	Client(String),
	/// Not rpc specific errors.
	#[display(fmt = "{}", _0)]
	Other(Box<dyn std::error::Error + Send>),
}

impl std::error::Error for RpcError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match *self {
			Self::JsonRpcError(ref e) => Some(e),
			Self::ParseError(_, ref e) => Some(&**e),
			Self::Other(ref e) => Some(&**e),
			_ => None,
		}
	}
}

impl From<Error> for RpcError {
	fn from(error: Error) -> Self {
		RpcError::JsonRpcError(error)
	}
}

/// A result returned by the client.
pub type RpcResult<T> = Result<T, RpcError>;

/// An RPC call message.
struct CallMessage {
	/// The RPC method name.
	method: String,
	/// The RPC method parameters.
	params: Params,
	/// The oneshot channel to send the result of the rpc
	/// call to.
	sender: oneshot::Sender<RpcResult<Value>>,
}

/// An RPC notification.
struct NotifyMessage {
	/// The RPC method name.
	method: String,
	/// The RPC method paramters.
	params: Params,
}

/// An RPC subscription.
struct Subscription {
	/// The subscribe method name.
	subscribe: String,
	/// The subscribe method parameters.
	subscribe_params: Params,
	/// The name of the notification.
	notification: String,
	/// The unsubscribe method name.
	unsubscribe: String,
}

/// An RPC subscribe message.
struct SubscribeMessage {
	/// The subscription to subscribe to.
	subscription: Subscription,
	/// The channel to send notifications to.
	sender: mpsc::UnboundedSender<RpcResult<Value>>,
}

/// A message sent to the `RpcClient`.
enum RpcMessage {
	/// Make an RPC call.
	Call(CallMessage),
	/// Send a notification.
	Notify(NotifyMessage),
	/// Subscribe to a notification.
	Subscribe(SubscribeMessage),
}

impl From<CallMessage> for RpcMessage {
	fn from(msg: CallMessage) -> Self {
		RpcMessage::Call(msg)
	}
}

impl From<NotifyMessage> for RpcMessage {
	fn from(msg: NotifyMessage) -> Self {
		RpcMessage::Notify(msg)
	}
}

impl From<SubscribeMessage> for RpcMessage {
	fn from(msg: SubscribeMessage) -> Self {
		RpcMessage::Subscribe(msg)
	}
}

/// A channel to a `RpcClient`.
#[derive(Clone)]
pub struct RpcChannel(mpsc::UnboundedSender<RpcMessage>);

impl RpcChannel {
	fn send(&self, msg: RpcMessage) -> Result<(), mpsc::TrySendError<RpcMessage>> {
		self.0.unbounded_send(msg)
	}
}

impl From<mpsc::UnboundedSender<RpcMessage>> for RpcChannel {
	fn from(sender: mpsc::UnboundedSender<RpcMessage>) -> Self {
		RpcChannel(sender)
	}
}

/// The future returned by the rpc call.
pub type RpcFuture = oneshot::Receiver<Result<Value, RpcError>>;

/// The stream returned by a subscribe.
pub type SubscriptionStream = mpsc::UnboundedReceiver<Result<Value, RpcError>>;

/// A typed subscription stream.
pub struct TypedSubscriptionStream<T> {
	_marker: PhantomData<T>,
	returns: &'static str,
	stream: SubscriptionStream,
}

impl<T> TypedSubscriptionStream<T> {
	/// Creates a new `TypedSubscriptionStream`.
	pub fn new(stream: SubscriptionStream, returns: &'static str) -> Self {
		TypedSubscriptionStream {
			_marker: PhantomData,
			returns,
			stream,
		}
	}
}

impl<T: DeserializeOwned + Unpin + 'static> Stream for TypedSubscriptionStream<T> {
	type Item = RpcResult<T>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
		let result = futures::ready!(self.stream.poll_next_unpin(cx));
		match result {
			Some(Ok(value)) => Some(
				serde_json::from_value::<T>(value)
					.map_err(|error| RpcError::ParseError(self.returns.into(), Box::new(error))),
			),
			None => None,
			Some(Err(err)) => Some(Err(err.into())),
		}
		.into()
	}
}

/// Client for raw JSON RPC requests
#[derive(Clone)]
pub struct RawClient(RpcChannel);

impl From<RpcChannel> for RawClient {
	fn from(channel: RpcChannel) -> Self {
		RawClient(channel)
	}
}

impl RawClient {
	/// Call RPC method with raw JSON.
	pub fn call_method(&self, method: &str, params: Params) -> impl Future<Output = RpcResult<Value>> {
		let (sender, receiver) = oneshot::channel();
		let msg = CallMessage {
			method: method.into(),
			params,
			sender,
		};
		let result = self.0.send(msg.into());
		async move {
			let () = result.map_err(|e| RpcError::Other(Box::new(e)))?;

			receiver.await.map_err(|e| RpcError::Other(Box::new(e)))?
		}
	}

	/// Send RPC notification with raw JSON.
	pub fn notify(&self, method: &str, params: Params) -> RpcResult<()> {
		let msg = NotifyMessage {
			method: method.into(),
			params,
		};
		match self.0.send(msg.into()) {
			Ok(()) => Ok(()),
			Err(error) => Err(RpcError::Other(Box::new(error))),
		}
	}

	/// Subscribe to topic with raw JSON.
	pub fn subscribe(
		&self,
		subscribe: &str,
		subscribe_params: Params,
		notification: &str,
		unsubscribe: &str,
	) -> RpcResult<SubscriptionStream> {
		let (sender, receiver) = mpsc::unbounded();
		let msg = SubscribeMessage {
			subscription: Subscription {
				subscribe: subscribe.into(),
				subscribe_params,
				notification: notification.into(),
				unsubscribe: unsubscribe.into(),
			},
			sender,
		};

		self.0
			.send(msg.into())
			.map(|()| receiver)
			.map_err(|e| RpcError::Other(Box::new(e)))
	}
}

/// Client for typed JSON RPC requests
#[derive(Clone)]
pub struct TypedClient(RawClient);

impl From<RpcChannel> for TypedClient {
	fn from(channel: RpcChannel) -> Self {
		TypedClient(channel.into())
	}
}

impl TypedClient {
	/// Create a new `TypedClient`.
	pub fn new(raw_cli: RawClient) -> Self {
		TypedClient(raw_cli)
	}

	/// Call RPC with serialization of request and deserialization of response.
	pub fn call_method<T: Serialize, R: DeserializeOwned>(
		&self,
		method: &str,
		returns: &str,
		args: T,
	) -> impl Future<Output = RpcResult<R>> {
		let returns = returns.to_owned();
		let args =
			serde_json::to_value(args).expect("Only types with infallible serialisation can be used for JSON-RPC");
		let params = match args {
			Value::Array(vec) => Ok(Params::Array(vec)),
			Value::Null => Ok(Params::None),
			Value::Object(map) => Ok(Params::Map(map)),
			_ => Err(RpcError::Client(
				"RPC params should serialize to a JSON array, JSON object or null".into(),
			)),
		};
		let result = params.map(|params| self.0.call_method(method, params));

		async move {
			let value: Value = result?.await?;

			log::debug!("response: {:?}", value);

			serde_json::from_value::<R>(value).map_err(|error| RpcError::ParseError(returns, Box::new(error)))
		}
	}

	/// Call RPC with serialization of request only.
	pub fn notify<T: Serialize>(&self, method: &str, args: T) -> RpcResult<()> {
		let args =
			serde_json::to_value(args).expect("Only types with infallible serialisation can be used for JSON-RPC");
		let params = match args {
			Value::Array(vec) => Params::Array(vec),
			Value::Null => Params::None,
			_ => {
				return Err(RpcError::Client(
					"RPC params should serialize to a JSON array, or null".into(),
				))
			}
		};

		self.0.notify(method, params)
	}

	/// Subscribe with serialization of request and deserialization of response.
	pub fn subscribe<T: Serialize, R: DeserializeOwned + 'static>(
		&self,
		subscribe: &str,
		subscribe_params: T,
		topic: &str,
		unsubscribe: &str,
		returns: &'static str,
	) -> RpcResult<TypedSubscriptionStream<R>> {
		let args = serde_json::to_value(subscribe_params)
			.expect("Only types with infallible serialisation can be used for JSON-RPC");

		let params = match args {
			Value::Array(vec) => Params::Array(vec),
			Value::Null => Params::None,
			_ => {
				return Err(RpcError::Client(
					"RPC params should serialize to a JSON array, or null".into(),
				))
			}
		};

		self.0
			.subscribe(subscribe, params, topic, unsubscribe)
			.map(move |stream| TypedSubscriptionStream::new(stream, returns))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::transports::local;
	use crate::{RpcChannel, TypedClient};
	use jsonrpc_core::futures::{future, FutureExt};
	use jsonrpc_core::{self as core, IoHandler};
	use jsonrpc_pubsub::{PubSubHandler, Subscriber, SubscriptionId};
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::sync::Arc;

	#[derive(Clone)]
	struct AddClient(TypedClient);

	impl From<RpcChannel> for AddClient {
		fn from(channel: RpcChannel) -> Self {
			AddClient(channel.into())
		}
	}

	impl AddClient {
		fn add(&self, a: u64, b: u64) -> impl Future<Output = RpcResult<u64>> {
			self.0.call_method("add", "u64", (a, b))
		}

		fn completed(&self, success: bool) -> RpcResult<()> {
			self.0.notify("completed", (success,))
		}
	}

	#[test]
	fn test_client_terminates() {
		crate::logger::init_log();
		let mut handler = IoHandler::new();
		handler.add_sync_method("add", |params: Params| {
			let (a, b) = params.parse::<(u64, u64)>()?;
			let res = a + b;
			Ok(jsonrpc_core::to_value(res).unwrap())
		});

		let (tx, rx) = std::sync::mpsc::channel();
		let (client, rpc_client) = local::connect::<AddClient, _, _>(handler);
		let fut = async move {
			let res = client.add(3, 4).await?;
			let res = client.add(res, 5).await?;
			assert_eq!(res, 12);
			tx.send(()).unwrap();
			Ok(()) as RpcResult<_>
		};
		let pool = futures::executor::ThreadPool::builder().pool_size(1).create().unwrap();
		pool.spawn_ok(rpc_client.map(|x| x.unwrap()));
		pool.spawn_ok(fut.map(|x| x.unwrap()));
		rx.recv().unwrap()
	}

	#[test]
	fn should_send_notification() {
		crate::logger::init_log();
		let (tx, rx) = std::sync::mpsc::sync_channel(1);
		let mut handler = IoHandler::new();
		handler.add_notification("completed", move |params: Params| {
			let (success,) = params.parse::<(bool,)>().expect("expected to receive one boolean");
			assert_eq!(success, true);
			tx.send(()).unwrap();
		});

		let (client, rpc_client) = local::connect::<AddClient, _, _>(handler);
		client.completed(true).unwrap();
		let pool = futures::executor::ThreadPool::builder().pool_size(1).create().unwrap();
		pool.spawn_ok(rpc_client.map(|x| x.unwrap()));
		rx.recv().unwrap()
	}

	#[test]
	fn should_handle_subscription() {
		crate::logger::init_log();
		// given
		let (finish, finished) = std::sync::mpsc::sync_channel(1);
		let mut handler = PubSubHandler::<local::LocalMeta, _>::default();
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		handler.add_subscription(
			"hello",
			("subscribe_hello", move |params, _meta, subscriber: Subscriber| {
				assert_eq!(params, core::Params::None);
				let sink = subscriber
					.assign_id(SubscriptionId::Number(5))
					.expect("assigned subscription id");
				let finish = finish.clone();
				std::thread::spawn(move || {
					for i in 0..3 {
						std::thread::sleep(std::time::Duration::from_millis(100));
						let value = serde_json::json!({
							"subscription": 5,
							"result": vec![i],
						});
						let _ = sink.notify(serde_json::from_value(value).unwrap());
					}
					finish.send(()).unwrap();
				});
			}),
			("unsubscribe_hello", move |id, _meta| {
				// Should be called because session is dropped.
				called2.store(true, Ordering::SeqCst);
				assert_eq!(id, SubscriptionId::Number(5));
				future::ready(Ok(core::Value::Bool(true)))
			}),
		);

		// when
		let (tx, rx) = std::sync::mpsc::channel();
		let (client, rpc_client) = local::connect_with_pubsub::<TypedClient, _>(handler);
		let received = Arc::new(std::sync::Mutex::new(vec![]));
		let r2 = received.clone();
		let fut = async move {
			let mut stream =
				client.subscribe::<_, (u32,)>("subscribe_hello", (), "hello", "unsubscribe_hello", "u32")?;
			let result = stream.next().await;
			r2.lock().unwrap().push(result.expect("Expected at least one item."));
			tx.send(()).unwrap();
			Ok(()) as RpcResult<_>
		};

		let pool = futures::executor::ThreadPool::builder().pool_size(1).create().unwrap();
		pool.spawn_ok(rpc_client.map(|_| ()));
		pool.spawn_ok(fut.map(|x| x.unwrap()));

		rx.recv().unwrap();
		assert!(
			!received.lock().unwrap().is_empty(),
			"Expected at least one received item."
		);
		// The session is being dropped only when another notification is received.
		// TODO [ToDr] we should unsubscribe as soon as the stream is dropped instead!
		finished.recv().unwrap();
		assert_eq!(called.load(Ordering::SeqCst), true, "Unsubscribe not called.");
	}
}
