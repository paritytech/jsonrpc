//! JSON-RPC client implementation.

#![deny(missing_docs)]

use failure::{format_err, Fail};
use futures::sync::{mpsc, oneshot};
use futures::{future, prelude::*};
use jsonrpc_core::{Error, Params};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

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
	/// The server returned a response with an unknown id.
	#[fail(display = "Server returned a response with an unknown id")]
	UnknownId,
	/// Not rpc specific errors.
	#[fail(display = "{}", _0)]
	Other(failure::Error),
}

impl From<Error> for RpcError {
	fn from(error: Error) -> Self {
		RpcError::JsonRpcError(error)
	}
}

/// A rpc call message.
struct CallMessage {
	/// The rpc method name.
	method: String,
	/// The rpc method parameters.
	params: Params,
	/// The oneshot channel to send the result of the rpc
	/// call to.
	sender: oneshot::Sender<Result<Value, RpcError>>,
}

/// A rpc subscribe message.
struct SubscribeMessage {
	/// The subscribe method name.
	subscribe_method: String,
	/// The subscribe method parameters.
	subscribe_params: Params,
	/// The topic name.
	topic: String,
	/// The channel to send notifications to.
	sender: mpsc::Sender<Result<Value, RpcError>>,
	/// The unsubscribe method name.
	unsubscribe_method: String,
}

/// A message sent to the `RpcClient`.
enum RpcMessage {
	/// Make a rpc call.
	Call(CallMessage),
	/// Subscribe to a notification.
	Subscribe(SubscribeMessage),
}

impl From<CallMessage> for RpcMessage {
	fn from(msg: CallMessage) -> Self {
		RpcMessage::Call(msg)
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

/// Client for raw JSON RPC requests
#[derive(Clone)]
pub struct RawClient(RpcChannel);

impl From<RpcChannel> for RawClient {
	fn from(channel: RpcChannel) -> Self {
		RawClient(channel)
	}
}

impl RawClient {
	/// Call RPC with raw JSON
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
	/// Create new TypedClient
	pub fn new(raw_cli: RawClient) -> Self {
		TypedClient(raw_cli)
	}

	/// Call RPC with serialization of request and deserialization of response
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
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::transports::local;
	use crate::{RpcChannel, RpcError, TypedClient};
	use jsonrpc_core::{self, IoHandler};

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
}
