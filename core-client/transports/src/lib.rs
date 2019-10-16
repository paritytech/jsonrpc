//! JSON-RPC client implementation.

#![deny(missing_docs)]

use failure::{format_err, Fail};
use futures::sync::{mpsc, oneshot};
use futures::{future, prelude::*};
use jsonrpc_core::{Error, Params};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::marker::PhantomData;

pub mod transports;

#[cfg(test)]
mod logger;

/// The errors returned by the client.
#[derive(Debug, Fail)]
pub enum RpcError {
	/// An error returned by the server.
	#[fail(display = "Server returned rpc error {}", _0)]
	JsonRpcError(Error),
	/// Failure to parse server response.
	#[fail(display = "Failed to parse server response as {}: {}", _0, _1)]
	ParseError(String, failure::Error),
	/// Request timed out.
	#[fail(display = "Request timed out")]
	Timeout,
	/// Not rpc specific errors.
	#[fail(display = "{}", _0)]
	Other(failure::Error),
}

impl From<Error> for RpcError {
	fn from(error: Error) -> Self {
		RpcError::JsonRpcError(error)
	}
}

/// An RPC call message.
struct CallMessage {
	/// The RPC method name.
	method: String,
	/// The RPC method parameters.
	params: Params,
	/// The oneshot channel to send the result of the rpc
	/// call to.
	sender: oneshot::Sender<Result<Value, RpcError>>,
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
	sender: mpsc::Sender<Result<Value, RpcError>>,
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
pub struct RpcChannel(mpsc::Sender<RpcMessage>);

impl RpcChannel {
	fn send(
		&self,
		msg: RpcMessage,
	) -> impl Future<Item = mpsc::Sender<RpcMessage>, Error = mpsc::SendError<RpcMessage>> {
		self.0.to_owned().send(msg)
	}
}

impl From<mpsc::Sender<RpcMessage>> for RpcChannel {
	fn from(sender: mpsc::Sender<RpcMessage>) -> Self {
		RpcChannel(sender)
	}
}

/// The future returned by the rpc call.
pub struct RpcFuture {
	recv: oneshot::Receiver<Result<Value, RpcError>>,
}

impl RpcFuture {
	/// Creates a new `RpcFuture`.
	pub fn new(recv: oneshot::Receiver<Result<Value, RpcError>>) -> Self {
		RpcFuture { recv }
	}
}

impl Future for RpcFuture {
	type Item = Value;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
		// TODO should timeout (#410)
		match self.recv.poll() {
			Ok(Async::Ready(Ok(value))) => Ok(Async::Ready(value)),
			Ok(Async::Ready(Err(error))) => Err(error),
			Ok(Async::NotReady) => Ok(Async::NotReady),
			Err(error) => Err(RpcError::Other(error.into())),
		}
	}
}

/// The stream returned by a subscribe.
pub struct SubscriptionStream {
	recv: mpsc::Receiver<Result<Value, RpcError>>,
}

impl SubscriptionStream {
	/// Crates a new `SubscriptionStream`.
	pub fn new(recv: mpsc::Receiver<Result<Value, RpcError>>) -> Self {
		SubscriptionStream { recv }
	}
}

impl Stream for SubscriptionStream {
	type Item = Value;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		match self.recv.poll() {
			Ok(Async::Ready(Some(Ok(value)))) => Ok(Async::Ready(Some(value))),
			Ok(Async::Ready(Some(Err(error)))) => Err(error),
			Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
			Ok(Async::NotReady) => Ok(Async::NotReady),
			Err(()) => Err(RpcError::Other(format_err!("mpsc channel returned an error."))),
		}
	}
}

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

impl<T: DeserializeOwned + 'static> Stream for TypedSubscriptionStream<T> {
	type Item = T;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		let result = match self.stream.poll()? {
			Async::Ready(Some(value)) => serde_json::from_value::<T>(value)
				.map(|result| Async::Ready(Some(result)))
				.map_err(|error| RpcError::ParseError(self.returns.into(), error.into()))?,
			Async::Ready(None) => Async::Ready(None),
			Async::NotReady => Async::NotReady,
		};
		Ok(result)
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
	pub fn call_method(&self, method: &str, params: Params) -> impl Future<Item = Value, Error = RpcError> {
		let (sender, receiver) = oneshot::channel();
		let msg = CallMessage {
			method: method.into(),
			params,
			sender,
		};
		self.0
			.send(msg.into())
			.map_err(|error| RpcError::Other(error.into()))
			.and_then(|_| RpcFuture::new(receiver))
	}

	/// Send RPC notification with raw JSON.
	pub fn notify(&self, method: &str, params: Params) -> impl Future<Item = (), Error = RpcError> {
		let msg = NotifyMessage {
			method: method.into(),
			params,
		};
		self.0
			.send(msg.into())
			.map(|_| ())
			.map_err(|error| RpcError::Other(error.into()))
	}

	/// Subscribe to topic with raw JSON.
	pub fn subscribe(
		&self,
		subscribe: &str,
		subscribe_params: Params,
		notification: &str,
		unsubscribe: &str,
	) -> impl Future<Item = SubscriptionStream, Error = RpcError> {
		let (sender, receiver) = mpsc::channel(0);
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
			.map_err(|error| RpcError::Other(error.into()))
			.map(|_| SubscriptionStream::new(receiver))
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
	pub fn call_method<T: Serialize, R: DeserializeOwned + 'static>(
		&self,
		method: &str,
		returns: &'static str,
		args: T,
	) -> impl Future<Item = R, Error = RpcError> {
		let args =
			serde_json::to_value(args).expect("Only types with infallible serialisation can be used for JSON-RPC");
		let params = match args {
			Value::Array(vec) => Params::Array(vec),
			Value::Null => Params::None,
			_ => {
				return future::Either::A(future::err(RpcError::Other(format_err!(
					"RPC params should serialize to a JSON array, or null"
				))))
			}
		};

		future::Either::B(self.0.call_method(method, params).and_then(move |value: Value| {
			log::debug!("response: {:?}", value);
			let result =
				serde_json::from_value::<R>(value).map_err(|error| RpcError::ParseError(returns.into(), error.into()));
			future::done(result)
		}))
	}

	/// Call RPC with serialization of request only.
	pub fn notify<T: Serialize>(&self, method: &str, args: T) -> impl Future<Item = (), Error = RpcError> {
		let args =
			serde_json::to_value(args).expect("Only types with infallible serialisation can be used for JSON-RPC");
		let params = match args {
			Value::Array(vec) => Params::Array(vec),
			Value::Null => Params::None,
			_ => {
				return future::Either::A(future::err(RpcError::Other(format_err!(
					"RPC params should serialize to a JSON array, or null"
				))))
			}
		};

		future::Either::B(self.0.notify(method, params))
	}

	/// Subscribe with serialization of request and deserialization of response.
	pub fn subscribe<T: Serialize, R: DeserializeOwned + 'static>(
		&self,
		subscribe: &str,
		subscribe_params: T,
		topic: &str,
		unsubscribe: &str,
		returns: &'static str,
	) -> impl Future<Item = TypedSubscriptionStream<R>, Error = RpcError> {
		let args = serde_json::to_value(subscribe_params)
			.expect("Only types with infallible serialisation can be used for JSON-RPC");

		let params = match args {
			Value::Array(vec) => Params::Array(vec),
			Value::Null => Params::None,
			_ => {
				return future::Either::A(future::err(RpcError::Other(format_err!(
					"RPC params should serialize to a JSON array, or null"
				))))
			}
		};

		let typed_stream = self
			.0
			.subscribe(subscribe, params, topic, unsubscribe)
			.map(move |stream| TypedSubscriptionStream::new(stream, returns));
		future::Either::B(typed_stream)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::transports::local;
	use crate::{RpcChannel, RpcError, TypedClient};
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
		fn add(&self, a: u64, b: u64) -> impl Future<Item = u64, Error = RpcError> {
			self.0.call_method("add", "u64", (a, b))
		}

		fn completed(&self, success: bool) -> impl Future<Item = (), Error = RpcError> {
			self.0.notify("completed", (success,))
		}
	}

	#[test]
	fn test_client_terminates() {
		crate::logger::init_log();
		let mut handler = IoHandler::new();
		handler.add_method("add", |params: Params| {
			let (a, b) = params.parse::<(u64, u64)>()?;
			let res = a + b;
			Ok(jsonrpc_core::to_value(res).unwrap())
		});

		let (client, rpc_client) = local::connect::<AddClient, _, _>(handler);
		let fut = client
			.clone()
			.add(3, 4)
			.and_then(move |res| client.add(res, 5))
			.join(rpc_client)
			.map(|(res, ())| {
				assert_eq!(res, 12);
			})
			.map_err(|err| {
				eprintln!("{:?}", err);
				assert!(false);
			});
		tokio::run(fut);
	}

	#[test]
	fn should_send_notification() {
		crate::logger::init_log();
		let mut handler = IoHandler::new();
		handler.add_notification("completed", |params: Params| {
			let (success,) = params.parse::<(bool,)>().expect("expected to receive one boolean");
			assert_eq!(success, true);
		});

		let (client, rpc_client) = local::connect::<AddClient, _, _>(handler);
		let fut = client
			.clone()
			.completed(true)
			.map(move |()| drop(client))
			.join(rpc_client)
			.map(|_| ())
			.map_err(|err| {
				eprintln!("{:?}", err);
				assert!(false);
			});
		tokio::run(fut);
	}

	#[test]
	fn should_handle_subscription() {
		crate::logger::init_log();
		// given
		let mut handler = PubSubHandler::<local::LocalMeta, _>::default();
		let called = Arc::new(AtomicBool::new(false));
		let called2 = called.clone();
		handler.add_subscription(
			"hello",
			("subscribe_hello", |params, _meta, subscriber: Subscriber| {
				assert_eq!(params, core::Params::None);
				let sink = subscriber
					.assign_id(SubscriptionId::Number(5))
					.expect("assigned subscription id");
				std::thread::spawn(move || {
					for i in 0..3 {
						std::thread::sleep(std::time::Duration::from_millis(100));
						let value = serde_json::json!({
							"subscription": 5,
							"result": vec![i],
						});
						sink.notify(serde_json::from_value(value).unwrap())
							.wait()
							.expect("sent notification");
					}
				});
			}),
			("unsubscribe_hello", move |id, _meta| {
				// Should be called because session is dropped.
				called2.store(true, Ordering::SeqCst);
				assert_eq!(id, SubscriptionId::Number(5));
				future::ok(core::Value::Bool(true))
			}),
		);

		// when
		let (client, rpc_client) = local::connect_with_pubsub::<TypedClient, _>(handler);
		let received = Arc::new(std::sync::Mutex::new(vec![]));
		let r2 = received.clone();
		let fut = client
			.subscribe::<_, (u32,)>("subscribe_hello", (), "hello", "unsubscribe_hello", "u32")
			.and_then(|stream| {
				stream
					.into_future()
					.map(move |(result, _)| {
						drop(client);
						r2.lock().unwrap().push(result.unwrap());
					})
					.map_err(|_| {
						panic!("Expected message not received.");
					})
			})
			.join(rpc_client)
			.map(|(res, _)| {
				log::info!("ok {:?}", res);
			})
			.map_err(|err| {
				log::error!("err {:?}", err);
			});
		tokio::run(fut);
		assert_eq!(called.load(Ordering::SeqCst), true);
		assert!(
			!received.lock().unwrap().is_empty(),
			"Expected at least one received item."
		);
	}
}
