//! JSON-RPC websocket client implementation.
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{RpcChannel, RpcError};
use websocket::{ClientBuilder, OwnedMessage};

/// Connect to a JSON-RPC websocket server.
///
/// Uses an unbounded channel to queue outgoing rpc messages.
///
/// Returns `Err` if the `url` is invalid.
pub fn try_connect<T>(url: &str) -> Result<impl Future<Output = Result<T, RpcError>>, RpcError>
where
	T: From<RpcChannel>,
{
	let client_builder = ClientBuilder::new(url).map_err(|e| RpcError::Other(Box::new(e)))?;
	Ok(do_connect(client_builder))
}

/// Connect to a JSON-RPC websocket server.
///
/// Uses an unbounded channel to queue outgoing rpc messages.
pub fn connect<T>(url: &url::Url) -> impl Future<Output = Result<T, RpcError>>
where
	T: From<RpcChannel>,
{
	let client_builder = ClientBuilder::from_url(url);
	do_connect(client_builder)
}

fn do_connect<T>(client_builder: ClientBuilder) -> impl Future<Output = Result<T, RpcError>>
where
	T: From<RpcChannel>,
{
	use futures::compat::{Future01CompatExt, Sink01CompatExt, Stream01CompatExt};
	use futures::{SinkExt, StreamExt, TryFutureExt, TryStreamExt};
	use websocket::futures::Stream;

	client_builder
		.async_connect(None)
		.compat()
		.map_err(|error| RpcError::Other(Box::new(error)))
		.map_ok(|(client, _)| {
			let (sink, stream) = client.split();

			let sink = sink.sink_compat().sink_map_err(|e| RpcError::Other(Box::new(e)));
			let stream = stream.compat().map_err(|e| RpcError::Other(Box::new(e)));
			let (sink, stream) = WebsocketClient::new(sink, stream).split();
			let (sink, stream) = (
				Box::pin(sink),
				Box::pin(
					stream
						.take_while(|x| futures::future::ready(x.is_ok()))
						.map(|x| x.expect("Stream is closed upon first error.")),
				),
			);
			let (rpc_client, sender) = super::duplex(sink, stream);
			let rpc_client = rpc_client.map_err(|error| log::error!("{:?}", error));
			tokio::spawn(rpc_client);

			sender.into()
		})
}

struct WebsocketClient<TSink, TStream> {
	sink: TSink,
	stream: TStream,
	queue: VecDeque<OwnedMessage>,
}

impl<TSink, TStream, TError> WebsocketClient<TSink, TStream>
where
	TSink: futures::Sink<OwnedMessage, Error = TError>,
	TStream: futures::Stream<Item = Result<OwnedMessage, TError>>,
	TError: std::error::Error + Send + 'static,
{
	pub fn new(sink: TSink, stream: TStream) -> Self {
		Self {
			sink,
			stream,
			queue: VecDeque::new(),
		}
	}
}

impl<TSink, TStream> futures::Sink<String> for WebsocketClient<TSink, TStream>
where
	TSink: futures::Sink<OwnedMessage, Error = RpcError> + Unpin,
	TStream: futures::Stream<Item = Result<OwnedMessage, RpcError>> + Unpin,
{
	type Error = RpcError;

	fn start_send(mut self: Pin<&mut Self>, request: String) -> Result<(), Self::Error> {
		self.queue.push_back(OwnedMessage::Text(request));
		Ok(())
	}

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		let this = Pin::into_inner(self);

		loop {
			match this.queue.pop_front() {
				Some(request) => match Pin::new(&mut this.sink).poll_ready(cx) {
					Poll::Ready(Ok(())) => {
						if let Err(err) = Pin::new(&mut this.sink).start_send(request) {
							return Poll::Ready(Err(RpcError::Other(Box::new(err))));
						}
						continue;
					}
					poll => {
						this.queue.push_front(request);
						return poll;
					}
				},
				None => break,
			}
		}

		Pin::new(&mut this.sink)
			.poll_ready(cx)
			.map_err(|error| RpcError::Other(Box::new(error)))
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		if !self.queue.is_empty() {
			self.poll_ready(cx)
		} else {
			let this = Pin::into_inner(self);
			Pin::new(&mut this.sink).poll_flush(cx)
		}
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		if !self.queue.is_empty() {
			self.poll_ready(cx)
		} else {
			let this = Pin::into_inner(self);
			Pin::new(&mut this.sink).poll_close(cx)
		}
	}
}

impl<TSink, TStream> futures::Stream for WebsocketClient<TSink, TStream>
where
	TSink: futures::Sink<OwnedMessage, Error = RpcError> + Unpin,
	TStream: futures::Stream<Item = Result<OwnedMessage, RpcError>> + Unpin,
{
	type Item = Result<String, RpcError>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = Pin::into_inner(self);
		loop {
			match Pin::new(&mut this.stream).poll_next(cx) {
				Poll::Ready(Some(Ok(message))) => match message {
					OwnedMessage::Text(data) => return Poll::Ready(Some(Ok(data))),
					OwnedMessage::Binary(data) => log::info!("server sent binary data {:?}", data),
					OwnedMessage::Ping(p) => this.queue.push_front(OwnedMessage::Pong(p)),
					OwnedMessage::Pong(_) => {}
					OwnedMessage::Close(c) => this.queue.push_front(OwnedMessage::Close(c)),
				},
				Poll::Ready(None) => {
					// TODO try to reconnect (#411).
					return Poll::Ready(None);
				}
				Poll::Pending => return Poll::Pending,
				Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(RpcError::Other(Box::new(error))))),
			}
		}
	}
}
