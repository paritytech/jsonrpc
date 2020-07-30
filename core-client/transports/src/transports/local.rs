//! Rpc client implementation for `Deref<Target=MetaIoHandler<Metadata>>`.

use crate::{RpcChannel, RpcError};
use failure::format_err;
use futures::prelude::*;
use futures::sync::mpsc;
use jsonrpc_core::{MetaIoHandler, Metadata};
use jsonrpc_pubsub::Session;
use std::ops::Deref;
use std::sync::Arc;

/// Implements a rpc client for `MetaIoHandler`.
pub struct LocalRpc<THandler, TMetadata> {
	handler: THandler,
	meta: TMetadata,
	sender: mpsc::UnboundedSender<String>,
	receiver: mpsc::UnboundedReceiver<String>,
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
		let (sender, receiver) = mpsc::unbounded();
		Self {
			handler,
			meta,
			sender,
			receiver,
		}
	}
}

impl<TMetadata, THandler> Stream for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	type Item = String;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		self.receiver
			.poll()
			.map_err(|()| RpcError::Other(format_err!("Sender dropped.")))
	}
}

impl<TMetadata, THandler> Sink for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	type SinkItem = String;
	type SinkError = RpcError;

	fn start_send(&mut self, request: Self::SinkItem) -> Result<AsyncSink<Self::SinkItem>, Self::SinkError> {
		match self.handler.handle_request_sync(&request, self.meta.clone()) {
			Some(response) => self
				.sender
				.unbounded_send(response)
				.map_err(|_| RpcError::Other(format_err!("Receiver dropped.")))?,
			None => {}
		};
		Ok(AsyncSink::Ready)
	}

	fn poll_complete(&mut self) -> Result<Async<()>, Self::SinkError> {
		Ok(Async::Ready(()))
	}
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata>`.
pub fn connect_with_metadata<TClient, THandler, TMetadata>(
	handler: THandler,
	meta: TMetadata,
) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
	TMetadata: Metadata,
{
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let (rpc_client, sender) = crate::transports::duplex(sink, stream);
	let client = TClient::from(sender);
	(client, rpc_client)
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata + Default>`.
pub fn connect<TClient, THandler, TMetadata>(handler: THandler) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
	TMetadata: Metadata + Default,
{
	connect_with_metadata(handler, Default::default())
}

/// Metadata for LocalRpc.
pub type LocalMeta = Arc<Session>;

/// Connects with pubsub.
pub fn connect_with_pubsub<TClient, THandler>(handler: THandler) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<LocalMeta>>,
{
	let (tx, rx) = mpsc::channel(0);
	let meta = Arc::new(Session::new(tx));
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let stream = stream.select(rx.map_err(|_| RpcError::Other(format_err!("Pubsub channel returned an error"))));
	let (rpc_client, sender) = crate::transports::duplex(sink, stream);
	let client = TClient::from(sender);
	(client, rpc_client)
}
