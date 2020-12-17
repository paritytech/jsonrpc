use std;
use std::net::SocketAddr;
use std::sync::Arc;

use tower_service::Service as _;

use futures01::sync::oneshot;
use futures01::{future, Future, Sink, Stream};

use crate::jsonrpc::{middleware, MetaIoHandler, Metadata, Middleware};
use crate::server_utils::{codecs, reactor, tokio02, tokio_util::codec::Framed, SuspendableStream};

use crate::dispatch::{Dispatcher, PeerMessageQueue, SenderChannels};
use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
use crate::service::Service;

/// TCP server builder
pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	executor: reactor::UninitializedExecutor,
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn MetaExtractor<M>>,
	channels: Arc<SenderChannels>,
	incoming_separator: codecs::Separator,
	outgoing_separator: codecs::Separator,
}

impl<M: Metadata + Default, S: Middleware<M> + 'static> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new `ServerBuilder` wih given `IoHandler`
	pub fn new<T>(handler: T) -> Self
	where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_meta_extractor(handler, NoopExtractor)
	}
}

impl<M: Metadata, S: Middleware<M> + 'static> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new `ServerBuilder` wih given `IoHandler`
	pub fn with_meta_extractor<T, E>(handler: T, extractor: E) -> Self
	where
		T: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M> + 'static,
	{
		ServerBuilder {
			executor: reactor::UninitializedExecutor::Unspawned,
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(extractor),
			channels: Default::default(),
			incoming_separator: Default::default(),
			outgoing_separator: Default::default(),
		}
	}

	// /// Utilize existing event loop executor.
	// pub fn event_loop_executor(mut self, handle: tokio_compat::runtime::TaskExecutor) -> Self {
	// 	self.executor = reactor::UninitializedExecutor::Shared(handle);
	// 	self
	// }

	/// Sets session meta extractor
	pub fn session_meta_extractor<T: MetaExtractor<M> + 'static>(mut self, meta_extractor: T) -> Self {
		self.meta_extractor = Arc::new(meta_extractor);
		self
	}

	/// Sets the incoming and outgoing requests separator
	pub fn request_separators(mut self, incoming: codecs::Separator, outgoing: codecs::Separator) -> Self {
		self.incoming_separator = incoming;
		self.outgoing_separator = outgoing;
		self
	}

	/// Starts a new server
	pub fn start(self, addr: &SocketAddr) -> std::io::Result<Server> {
		let meta_extractor = self.meta_extractor.clone();
		let rpc_handler = self.handler.clone();
		let channels = self.channels.clone();
		let incoming_separator = self.incoming_separator;
		let outgoing_separator = self.outgoing_separator;
		let address = addr.to_owned();
		let (tx, rx) = std::sync::mpsc::channel();
		let (stop_tx, stop_rx) = oneshot::channel();

		let executor = self.executor.initialize()?;

		use futures03::StreamExt;
		executor.executor().spawn(async move {
			let start = async move {
				let listener = tokio02::net::TcpListener::bind(&address).await?;
				let connections = SuspendableStream::new(listener);
				// let connections = connections.map(Ok);

				use futures03::TryStreamExt;
				let server = connections.map(move |socket| {
					let peer_addr = match socket.peer_addr() {
						Ok(addr) => addr,
						Err(e) => {
							warn!(target: "tcp", "Unable to determine socket peer address, ignoring connection {}", e);
							return future::Either::A(future::ok::<(), ()>(())).compat();
						}
					};
					trace!(target: "tcp", "Accepted incoming connection from {}", &peer_addr);
					let (sender, receiver) = crate::jsonrpc::futures::channel::mpsc::unbounded();

					let context = RequestContext {
						peer_addr,
						sender: sender.clone(),
					};

					let meta = meta_extractor.extract(&context);
					let mut service = Service::new(peer_addr, rpc_handler.clone(), meta);
					let (writer, reader) = Framed::new(
						socket,
						codecs::StreamCodec::new(incoming_separator.clone(), outgoing_separator.clone()),
					)
					.split();

					use futures03::TryStreamExt;
					use futures03::SinkExt;
					let reader = reader.compat();
					let writer = writer.compat();
					use futures03::TryFutureExt;
					let responses = reader.and_then(move |req| {
						service.call(req).compat().then(|response| match response {
							Err(e) => {
								warn!(target: "tcp", "Error while processing request: {:?}", e);
								future::ok(String::new())
							}
							Ok(None) => {
								trace!(target: "tcp", "JSON RPC request produced no response");
								future::ok(String::new())
							}
							Ok(Some(response_data)) => {
								trace!(target: "tcp", "Sent response: {}", &response_data);
								future::ok(response_data)
							}
						})
					});

					let peer_message_queue = {
						let mut channels = channels.lock();
						channels.insert(peer_addr, sender.clone());

						PeerMessageQueue::new(responses, receiver, peer_addr)
					};

					let shared_channels = channels.clone();
					let writer = writer.send_all(peer_message_queue).then(move |_| {
						trace!(target: "tcp", "Peer {}: service finished", peer_addr);
						let mut channels = shared_channels.lock();
						channels.remove(&peer_addr);
						Ok::<(), ()>(())
					});

					future::Either::B(writer).compat()
				});

				use futures03::compat::Stream01CompatExt;
				Ok(server)
			};

			use futures03::compat::Future01CompatExt;

			let stop = stop_rx.map(drop).map_err(drop).compat();
			match start.await {
				Ok(server) => {
					tx.send(Ok(())).expect("Rx is blocking parent thread.");
					let server = server
						.buffer_unordered(1024)
						.for_each(|_| async { () })
						;
					// let stop = stop;

					let select = futures03::future::select(Box::pin(server), stop);
					select.await;

					// future::Either::A(
					// 	server
					// 		.buffer_unordered(1024)
					// 		// .for_each(|_| async { Ok(()) })
					// 		.select(stop)
					// 		.map(|_| ())
					// 		.map_err(|(e, _)| {
					// 			error!("Error while executing the server: {:?}", e);
					// 		}),
					// )
				}
				Err(e) => {
					tx.send(Err(e)).expect("Rx is blocking parent thread.");
					stop.await;
					// future::Either::B(stop.map_err(|e| {
					// 	error!("Error while executing the server: {:?}", e);
					// }))
				}
			}
			// .compat()
			// .await
		});

		let res = rx.recv().expect("Response is always sent before tx is dropped.");

		res.map(|_| Server {
			executor: Some(executor),
			stop: Some(stop_tx),
		})
	}

	/// Returns dispatcher
	pub fn dispatcher(&self) -> Dispatcher {
		Dispatcher::new(self.channels.clone())
	}
}

/// TCP Server handle
pub struct Server {
	executor: Option<reactor::Executor>,
	stop: Option<oneshot::Sender<()>>,
}

impl Server {
	/// Closes the server (waits for finish)
	pub fn close(mut self) {
		let _ = self.stop.take().map(|sg| sg.send(()));
		self.executor.take().unwrap().close();
	}

	/// Wait for the server to finish
	pub fn wait(mut self) {
		self.executor.take().unwrap().wait();
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.stop.take().map(|sg| sg.send(()));
		if let Some(executor) = self.executor.take() {
			executor.close()
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn server_is_send_and_sync() {
		fn is_send_and_sync<T: Send + Sync>() {}

		is_send_and_sync::<Server>();
	}
}
