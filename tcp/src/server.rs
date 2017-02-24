use std;
use std::thread;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use tokio_core::io::Io;
use tokio_service::Service as TokioService;

use jsonrpc::{MetaIoHandler, Metadata, Middleware, NoopMiddleware};
use jsonrpc::futures::{future, Future, Stream, Sink};
use jsonrpc::futures::sync::{mpsc, oneshot};

use dispatch::{Dispatcher, SenderChannels, PeerMessageQueue};
use line_codec::LineCodec;
use meta::{MetaExtractor, RequestContext, NoopExtractor};
use service::Service;

pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	channels: Arc<SenderChannels>,
}

impl<M: Metadata, S: Middleware<M> + 'static> ServerBuilder<M, S> {
	pub fn new<T>(handler: T) -> Self where
		T: Into<MetaIoHandler<M, S>>
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			channels: Default::default(),
		}
	}

	pub fn session_meta_extractor<T: MetaExtractor<M> + 'static>(mut self, meta_extractor: T) -> Self {
		self.meta_extractor = Arc::new(meta_extractor);
		self
	}

	pub fn start(self, addr: &SocketAddr) -> std::io::Result<Server> {
		let meta_extractor = self.meta_extractor.clone();
		let rpc_handler = self.handler.clone();
		let channels = self.channels.clone();
		let address = addr.to_owned();
		let (tx, rx) = std::sync::mpsc::sync_channel(1);
		let (signal, stop) = oneshot::channel();

		let handle = thread::spawn(move || {
			let start = move || {
				let core = Core::new()?;
				let handle = core.handle();

				let listener = TcpListener::bind(&address, &handle)?;
				let connections = listener.incoming();
				let server = connections.for_each(move |(socket, peer_addr)| {
					trace!(target: "tcp", "Accepted incoming connection from {}", &peer_addr);
					let (sender, receiver) = mpsc::channel(65536);

					let context = RequestContext {
						peer_addr: peer_addr,
						sender: sender.clone(),
					};

					let meta = meta_extractor.extract(&context);
					let service = Service::new(peer_addr, rpc_handler.clone(), meta);
					let (writer, reader) = socket.framed(LineCodec).split();

					let responses = reader.and_then(
						move |req| service.call(req).then(|response| match response {
							Err(e) => {
								warn!(target: "tcp", "Error while processing request: {:?}", e);
								future::ok(String::new())
							},
							Ok(None) => {
								trace!(target: "tcp", "JSON RPC request produced no response");
								future::ok(String::new())
							},
							Ok(Some(response_data)) => {
								trace!(target: "tcp", "Sent response: {}", &response_data);
								future::ok(response_data)
							}
						})
					);

					let peer_message_queue = {
						let mut channels = channels.lock();
						channels.insert(peer_addr.clone(), sender.clone());

						PeerMessageQueue::new(
							responses,
							receiver,
							peer_addr.clone(),
						)
					};

					let shared_channels = channels.clone();
					let server = writer.send_all(peer_message_queue).then(move |_| {
						trace!(target: "tcp", "Peer {}: service finished", peer_addr);
						let mut channels = shared_channels.lock();
						channels.remove(&peer_addr);
						Ok(())
					});

					handle.spawn(server);

					Ok(())
				});

				Ok((core, server))
			};

			match start() {
				Ok((mut core, server)) => {
					tx.send(Ok(())).unwrap();

					match core.run(server.select(stop.map_err(|_| std::io::ErrorKind::Interrupted.into()))) {
						Ok(_) => Ok(()),
						Err((e, _)) => Err(e),
					}
				},
				Err(e) => {
					tx.send(Err(e)).unwrap();
					Ok(())
				},
			}
		});

		let res = rx.recv().unwrap();

		res.map(|_| Server {
			handle: Some(handle),
			stop: Some(signal),
		})
	}

	pub fn dispatcher(&self) -> Dispatcher {
		Dispatcher::new(self.channels.clone())
	}
}

pub struct Server {
	handle: Option<thread::JoinHandle<std::io::Result<()>>>,
	stop: Option<oneshot::Sender<()>>,
}

impl Server {
	pub fn close(mut self) -> std::io::Result<()> {
		self.stop.take().unwrap().complete(());
		self.wait()
	}

	pub fn wait(mut self) -> std::io::Result<()> {
		self.handle.take().unwrap().join().unwrap()
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.stop.take().map(|stop| stop.complete(()));
	}
}
