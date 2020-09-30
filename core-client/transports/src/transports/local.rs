//! Rpc client implementation for `Deref<Target=MetaIoHandler<Metadata>>`.

use crate::{RpcChannel, RpcError};
use failure::format_err;
use futures::prelude::*;
use futures::sync::mpsc;
use jsonrpc_core::{MetaIoHandler, Metadata, Middleware};
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

impl<TMetadata, THandler, TMiddleware> LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	TMiddleware: Middleware<TMetadata>,
	THandler: Deref<Target = MetaIoHandler<TMetadata, TMiddleware>>,
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

impl<TMetadata, THandler, TMiddleware> Stream for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	TMiddleware: Middleware<TMetadata>,
	THandler: Deref<Target = MetaIoHandler<TMetadata, TMiddleware>>,
{
	type Item = String;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		self.receiver
			.poll()
			.map_err(|()| RpcError::Other(format_err!("Sender dropped.")))
	}
}

impl<TMetadata, THandler, TMiddleware> Sink for LocalRpc<THandler, TMetadata>
where
	TMetadata: Metadata,
	TMiddleware: Middleware<TMetadata>,
	THandler: Deref<Target = MetaIoHandler<TMetadata, TMiddleware>>,
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

/// Connects to a `Deref<Target = MetaIoHandler<Metadata>` specifying a custom middleware implementation.
pub fn connect_with_metadata_and_middleware<TClient, THandler, TMetadata, TMiddleware>(
	handler: THandler,
	meta: TMetadata,
) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	TMiddleware: Middleware<TMetadata>,
	THandler: Deref<Target = MetaIoHandler<TMetadata, TMiddleware>>,
	TMetadata: Metadata,
{
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let (rpc_client, sender) = crate::transports::duplex(sink, stream);
	let client = TClient::from(sender);
	(client, rpc_client)
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
	connect_with_metadata_and_middleware(handler, meta)
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata + Default>` specifying a custom middleware implementation.
pub fn connect_with_middleware<TClient, THandler, TMetadata, TMiddleware>(
	handler: THandler,
) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	TMiddleware: Middleware<TMetadata>,
	THandler: Deref<Target = MetaIoHandler<TMetadata, TMiddleware>>,
	TMetadata: Metadata + Default,
{
	connect_with_metadata_and_middleware(handler, Default::default())
}

/// Connects to a `Deref<Target = MetaIoHandler<Metadata + Default>`.
pub fn connect<TClient, THandler, TMetadata>(handler: THandler) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
	TMetadata: Metadata + Default,
{
	connect_with_middleware(handler)
}

/// Metadata for LocalRpc.
pub type LocalMeta = Arc<Session>;

/// Connects with pubsub specifying a custom middleware implementation.
pub fn connect_with_pubsub_and_middleware<TClient, THandler, TMiddleware>(
	handler: THandler,
) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	TMiddleware: Middleware<LocalMeta>,
	THandler: Deref<Target = MetaIoHandler<LocalMeta, TMiddleware>>,
{
	let (tx, rx) = mpsc::channel(0);
	let meta = Arc::new(Session::new(tx));
	let (sink, stream) = LocalRpc::with_metadata(handler, meta).split();
	let stream = stream.select(rx.map_err(|_| RpcError::Other(format_err!("Pubsub channel returned an error"))));
	let (rpc_client, sender) = crate::transports::duplex(sink, stream);
	let client = TClient::from(sender);
	(client, rpc_client)
}

/// Connects with pubsub.
pub fn connect_with_pubsub<TClient, THandler>(handler: THandler) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	THandler: Deref<Target = MetaIoHandler<LocalMeta>>,
{
	connect_with_pubsub_and_middleware(handler)
}
