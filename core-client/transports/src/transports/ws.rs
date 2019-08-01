//! JSON-RPC websocket client implementation.
use crate::{RpcChannel, RpcError};
use failure::Error;
use futures::prelude::*;
use log::info;
use std::collections::VecDeque;
use websocket::{ClientBuilder, OwnedMessage};

/// Connect to a JSON-RPC websocket server.
///
/// Uses an unbuffered channel to queue outgoing rpc messages.
///
/// Returns `Err` if the `url` is invalid.
pub fn try_connect<T>(url: &str) -> Result<impl Future<Item = T, Error = RpcError>, Error>
where
	T: From<RpcChannel>,
{
	let client_builder = ClientBuilder::new(url)?;
	Ok(do_connect(client_builder))
}

/// Connect to a JSON-RPC websocket server.
///
/// Uses an unbuffered channel to queue outgoing rpc messages.
pub fn connect<T>(url: &url::Url) -> impl Future<Item = T, Error = RpcError>
where
	T: From<RpcChannel>,
{
	let client_builder = ClientBuilder::from_url(url);
	do_connect(client_builder)
}

fn do_connect<T>(client_builder: ClientBuilder) -> impl Future<Item = T, Error = RpcError>
where
	T: From<RpcChannel>,
{
	client_builder
		.async_connect(None)
		.map(|(client, _)| {
			let (sink, stream) = client.split();
			let (sink, stream) = WebsocketClient::new(sink, stream).split();
			let (rpc_client, sender) = super::duplex(sink, stream);
			let rpc_client = rpc_client.map_err(|error| eprintln!("{:?}", error));
			tokio::spawn(rpc_client);
			sender.into()
		})
		.map_err(|error| RpcError::Other(error.into()))
}

struct WebsocketClient<TSink, TStream> {
	sink: TSink,
	stream: TStream,
	queue: VecDeque<OwnedMessage>,
}

impl<TSink, TStream, TError> WebsocketClient<TSink, TStream>
where
	TSink: Sink<SinkItem = OwnedMessage, SinkError = TError>,
	TStream: Stream<Item = OwnedMessage, Error = TError>,
	TError: Into<Error>,
{
	pub fn new(sink: TSink, stream: TStream) -> Self {
		Self {
			sink,
			stream,
			queue: VecDeque::new(),
		}
	}
}

impl<TSink, TStream, TError> Sink for WebsocketClient<TSink, TStream>
where
	TSink: Sink<SinkItem = OwnedMessage, SinkError = TError>,
	TStream: Stream<Item = OwnedMessage, Error = TError>,
	TError: Into<Error>,
{
	type SinkItem = String;
	type SinkError = RpcError;

	fn start_send(&mut self, request: Self::SinkItem) -> Result<AsyncSink<Self::SinkItem>, Self::SinkError> {
		self.queue.push_back(OwnedMessage::Text(request));
		Ok(AsyncSink::Ready)
	}

	fn poll_complete(&mut self) -> Result<Async<()>, Self::SinkError> {
		loop {
			match self.queue.pop_front() {
				Some(request) => match self.sink.start_send(request) {
					Ok(AsyncSink::Ready) => continue,
					Ok(AsyncSink::NotReady(request)) => {
						self.queue.push_front(request);
						break;
					}
					Err(error) => return Err(RpcError::Other(error.into())),
				},
				None => break,
			}
		}
		self.sink.poll_complete().map_err(|error| RpcError::Other(error.into()))
	}
}

impl<TSink, TStream, TError> Stream for WebsocketClient<TSink, TStream>
where
	TSink: Sink<SinkItem = OwnedMessage, SinkError = TError>,
	TStream: Stream<Item = OwnedMessage, Error = TError>,
	TError: Into<Error>,
{
	type Item = String;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		loop {
			match self.stream.poll() {
				Ok(Async::Ready(Some(message))) => match message {
					OwnedMessage::Text(data) => return Ok(Async::Ready(Some(data))),
					OwnedMessage::Binary(data) => info!("server sent binary data {:?}", data),
					OwnedMessage::Ping(p) => self.queue.push_front(OwnedMessage::Pong(p)),
					OwnedMessage::Pong(_) => {}
					OwnedMessage::Close(c) => self.queue.push_front(OwnedMessage::Close(c)),
				},
				Ok(Async::Ready(None)) => {
					// TODO try to reconnect (#411).
					return Ok(Async::Ready(None));
				}
				Ok(Async::NotReady) => return Ok(Async::NotReady),
				Err(error) => return Err(RpcError::Other(error.into())),
			}
		}
	}
}
