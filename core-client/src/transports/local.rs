//! Rpc client implementation for `Deref<Target=MetaIoHandler<Metadata + Default>>`.

use crate::{RpcChannel, RpcError};
use futures::prelude::*;
use jsonrpc_core::{MetaIoHandler, Metadata};
use std::collections::VecDeque;
use std::ops::Deref;

/// Implements a rpc client for `MetaIoHandler`.
pub struct LocalRpc<THandler> {
	handler: THandler,
	queue: VecDeque<String>,
}

impl<TMetadata, THandler> LocalRpc<THandler>
where
	TMetadata: Metadata + Default,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	/// Creates a new `LocalRpc`.
	pub fn new(handler: THandler) -> Self {
		Self {
			handler,
			queue: VecDeque::new(),
		}
	}
}

impl<TMetadata, THandler> Stream for LocalRpc<THandler>
where
	TMetadata: Metadata + Default,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	type Item = String;
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
		match self.queue.pop_front() {
			Some(response) => Ok(Async::Ready(Some(response))),
			None => Ok(Async::NotReady),
		}
	}
}

impl<TMetadata, THandler> Sink for LocalRpc<THandler>
where
	TMetadata: Metadata + Default,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	type SinkItem = String;
	type SinkError = RpcError;

	fn start_send(&mut self, request: Self::SinkItem) -> Result<AsyncSink<Self::SinkItem>, Self::SinkError> {
		match self.handler.handle_request_sync(&request, TMetadata::default()) {
			Some(response) => self.queue.push_back(response),
			None => {}
		};
		Ok(AsyncSink::Ready)
	}

	fn poll_complete(&mut self) -> Result<Async<()>, Self::SinkError> {
		Ok(Async::Ready(()))
	}
}

/// Connects to a `IoHandler`.
pub fn connect<TClient, TMetadata, THandler>(handler: THandler) -> (TClient, impl Future<Item = (), Error = RpcError>)
where
	TClient: From<RpcChannel>,
	TMetadata: Metadata + Default,
	THandler: Deref<Target = MetaIoHandler<TMetadata>>,
{
	let (sink, stream) = LocalRpc::new(handler).split();
	let (rpc_client, sender) = crate::transports::Duplex::with_channel(sink, stream);
	let client = TClient::from(sender);
	(client, rpc_client)
}
