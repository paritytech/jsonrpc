//! Rpc client implementation for `Deref<Target=MetaIoHandler<Metadata>>`.

use crate::{RpcChannel, RpcError, RpcResult};
use futures::channel::mpsc;
use futures::{Stream, StreamExt, Sink, SinkExt, Future, task::{Context, Poll}};
use jsonrpc_core::{MetaIoHandler, Metadata, BoxFuture};
use jsonrpc_pubsub::Session;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;

/// Implements a rpc client for `MetaIoHandler`.
pub struct LocalRpc<THandler, TMetadata> {
	handler: THandler,
	meta: TMetadata,
	buffered: Buffered,
	queue: (mpsc::UnboundedSender<String>, mpsc::UnboundedReceiver<String>),
}

enum Buffered {
	Request(BoxFuture<Option<String>>),
	Response(String),
	None,
}


impl<TMetadata, THandler> LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	/// Creates a new `LocalRpc` with default metadata.
	pub fn new(handler: THandler) -> Self
	where
		TMetadata: Default,
	{
		Self::with_metadata(handler, Default::default())
	}

	/// Creates a new `LocalRpc` with given handler and metadata.
	pub fn with_metadata(handler: THandler, meta: TMetadata) -> Self {
		Self {
			handler,
			meta,
			buffered: Buffered::None,
			queue: mpsc::unbounded(),
		}
	}
}

impl<TMetadata, THandler> Stream for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata + Unpin,
	THandler: Deref<Target = MetaIoHandler<TMetadata>> + Unpin,
{
	type Item = String;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Self::Item>> {
		self.queue.1.poll_next_unpin(cx)
	}
}

impl<TMetadata, THandler> LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata + Unpin,
	THandler: Deref<Target = MetaIoHandler<TMetadata>> + Unpin,
{
	fn poll_buffered(&mut self, cx: &mut Context) -> Poll<Result<(), RpcError>> {
		let response = match self.buffered {
			Buffered::Request(ref mut r) => futures::ready!(r.as_mut().poll(cx)),
			_ => None,
		};
		if let Some(response) = response {
			self.buffered = Buffered::Response(response);
		}

		self.send_response().into()
	}

	fn send_response(&mut self) -> Result<(), RpcError> {
		if let Buffered::Response(r) = std::mem::replace(&mut self.buffered, Buffered::None) {
			self.queue.0.start_send(r)
				.map_err(|e| RpcError::Other(e.into()))?;
		}
		Ok(())
	}
}

impl<TMetadata, THandler> Sink<String> for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata + Unpin,
	THandler: Deref<Target = MetaIoHandler<TMetadata>> + Unpin,
{
    type Error = RpcError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		futures::ready!(self.poll_buffered(cx))?;
		futures::ready!(self.queue.0.poll_ready(cx))
			.map_err(|e| RpcError::Other(e.into()))
			.into()
	}

    fn start_send(mut self: Pin<&mut Self>, item: String) -> Result<(), Self::Error> {
		let future = self.handler.handle_request(&item, self.meta.clone());
		self.buffered = Buffered::Request(Box::pin(future));
		Ok(())
	}

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		futures::ready!(self.poll_buffered(cx))?;
		futures::ready!(self.queue.0.poll_flush_unpin(cx))
			.map_err(|e| RpcError::Other(e.into()))
			.into()
	}

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		futures::ready!(self.queue.0.poll_close_unpin(cx))
			.map_err(|e| RpcError::Other(e.into()))
			.into()
	}
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata>`.
pub fn connect_with_metadata<TClient, THandler, TMetadata>(
	handler: THandler,
	meta: TMetadata,
) -> (TClient, impl Future<Output = RpcResult<()>>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<TMetadata>> + Unpin,
	TMetadata: Metadata + Unpin,
{
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let (rpc_client, sender) = crate::transports::duplex(Box::pin(sink), Box::pin(stream));
	let client = TClient::from(sender);
	(client, rpc_client)
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata + Default>`.
pub fn connect<TClient, THandler, TMetadata>(
	handler: THandler,
) -> (TClient, impl Future<Output = RpcResult<()>>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<TMetadata>> + Unpin,
	TMetadata: Metadata + Default + Unpin,
{
	connect_with_metadata(handler, Default::default())
}

/// Metadata for LocalRpc.
pub type LocalMeta = Arc<Session>;

/// Connects with pubsub.
pub fn connect_with_pubsub<TClient, THandler>(
	handler: THandler,
) -> (TClient, impl Future<Output = RpcResult<()>>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<LocalMeta>> + Unpin,
{
	let (tx, rx) = mpsc::unbounded();
	let meta = Arc::new(Session::new(tx));
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let stream = futures::stream::select(
		stream,
		rx
	);
	let (rpc_client, sender) = crate::transports::duplex(Box::pin(sink), Box::pin(stream));
	let client = TClient::from(sender);
	(client, rpc_client)
}
