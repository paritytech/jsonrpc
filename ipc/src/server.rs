use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::jsonrpc::futures::channel::mpsc;
use crate::jsonrpc::{middleware, MetaIoHandler, Metadata, Middleware};
use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
use crate::select_with_weak::SelectWithWeakExt;
use futures::channel::oneshot;
use futures::StreamExt;
use parity_tokio_ipc::Endpoint;
use parking_lot::Mutex;
use tower_service::Service as _;

use crate::server_utils::{codecs, reactor, reactor::TaskExecutor, session, tokio_util};

pub use parity_tokio_ipc::SecurityAttributes;

/// IPC server session
pub struct Service<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
	/// Create new IPC server session with given handler and metadata.
	pub fn new(handler: Arc<MetaIoHandler<M, S>>, meta: M) -> Self {
		Service { handler, meta }
	}
}

impl<M: Metadata, S: Middleware<M>> tower_service::Service<String> for Service<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	type Response = Option<String>;
	type Error = ();

	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

	fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}

	fn call(&mut self, req: String) -> Self::Future {
		use futures::FutureExt;
		trace!(target: "ipc", "Received request: {}", req);
		Box::pin(self.handler.handle_request(&req, self.meta.clone()).map(Ok))
	}
}

/// IPC server builder
pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn MetaExtractor<M>>,
	session_stats: Option<Arc<dyn session::SessionStats>>,
	executor: reactor::UninitializedExecutor,
	incoming_separator: codecs::Separator,
	outgoing_separator: codecs::Separator,
	security_attributes: SecurityAttributes,
	client_buffer_size: usize,
}

impl<M: Metadata + Default, S: Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new IPC server build given the `IoHandler`.
	pub fn new<T>(io_handler: T) -> ServerBuilder<M, S>
	where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_meta_extractor(io_handler, NoopExtractor)
	}
}

impl<M: Metadata, S: Middleware<M>> ServerBuilder<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	/// Creates new IPC server build given the `IoHandler` and metadata extractor.
	pub fn with_meta_extractor<T, E>(io_handler: T, extractor: E) -> ServerBuilder<M, S>
	where
		T: Into<MetaIoHandler<M, S>>,
		E: MetaExtractor<M>,
	{
		ServerBuilder {
			handler: Arc::new(io_handler.into()),
			meta_extractor: Arc::new(extractor),
			session_stats: None,
			executor: reactor::UninitializedExecutor::Unspawned,
			incoming_separator: codecs::Separator::Empty,
			outgoing_separator: codecs::Separator::default(),
			security_attributes: SecurityAttributes::empty(),
			client_buffer_size: 5,
		}
	}

	/// Sets shared different event loop executor.
	pub fn event_loop_executor(mut self, executor: TaskExecutor) -> Self {
		self.executor = reactor::UninitializedExecutor::Shared(executor);
		self
	}

	/// Sets session metadata extractor.
	pub fn session_meta_extractor<X>(mut self, meta_extractor: X) -> Self
	where
		X: MetaExtractor<M>,
	{
		self.meta_extractor = Arc::new(meta_extractor);
		self
	}

	/// Session stats
	pub fn session_stats<T: session::SessionStats>(mut self, stats: T) -> Self {
		self.session_stats = Some(Arc::new(stats));
		self
	}

	/// Sets the incoming and outgoing requests separator
	pub fn request_separators(mut self, incoming: codecs::Separator, outgoing: codecs::Separator) -> Self {
		self.incoming_separator = incoming;
		self.outgoing_separator = outgoing;
		self
	}

	/// Sets the security attributes for the underlying IPC socket/pipe
	pub fn set_security_attributes(mut self, attr: SecurityAttributes) -> Self {
		self.security_attributes = attr;
		self
	}

	/// Sets how many concurrent requests per client can be processed at any one time. Set to 5 by default.
	pub fn set_client_buffer_size(mut self, buffer_size: usize) -> Self {
		self.client_buffer_size = buffer_size;
		self
	}

	/// Creates a new server from the given endpoint.
	pub fn start(self, path: &str) -> std::io::Result<Server> {
		let executor = self.executor.initialize()?;
		let rpc_handler = self.handler;
		let endpoint_addr = path.to_owned();
		let meta_extractor = self.meta_extractor;
		let session_stats = self.session_stats;
		let incoming_separator = self.incoming_separator;
		let outgoing_separator = self.outgoing_separator;
		let (stop_signal, stop_receiver) = oneshot::channel();
		// NOTE: These channels are only waited upon in synchronous fashion
		let (start_signal, start_receiver) = std::sync::mpsc::channel();
		let (wait_signal, wait_receiver) = std::sync::mpsc::channel();
		let security_attributes = self.security_attributes;
		let client_buffer_size = self.client_buffer_size;

		let fut = async move {
			let mut endpoint = Endpoint::new(endpoint_addr);
			endpoint.set_security_attributes(security_attributes);

			if cfg!(unix) {
				// warn about existing file and remove it
				if ::std::fs::remove_file(endpoint.path()).is_ok() {
					warn!("Removed existing file '{}'.", endpoint.path());
				}
			}

			let endpoint_addr = endpoint.path().to_owned();
			let connections = match endpoint.incoming() {
				Ok(connections) => connections,
				Err(e) => {
					start_signal
						.send(Err(e))
						.expect("Cannot fail since receiver never dropped before receiving");
					return;
				}
			};

			let mut id = 0u64;

			use futures::TryStreamExt;
			let server = connections.map_ok(move |io_stream| {
				id = id.wrapping_add(1);
				let session_id = id;
				let session_stats = session_stats.clone();
				trace!(target: "ipc", "Accepted incoming IPC connection: {}", session_id);
				if let Some(stats) = session_stats.as_ref() {
					stats.open_session(session_id)
				}

				let (sender, receiver) = mpsc::unbounded();
				let meta = meta_extractor.extract(&RequestContext {
					endpoint_addr: endpoint_addr.as_ref(),
					session_id,
					sender,
				});
				let mut service = Service::new(rpc_handler.clone(), meta);
				let codec = codecs::StreamCodec::new(incoming_separator.clone(), outgoing_separator.clone());
				let framed = tokio_util::codec::Decoder::framed(codec, io_stream);
				let (writer, reader) = futures::StreamExt::split(framed);

				let responses = reader
					.map_ok(move |req| {
						service
							.call(req)
							// Ignore service errors
							.map(|x| Ok(x.ok().flatten()))
					})
					.try_buffer_unordered(client_buffer_size)
					// Filter out previously ignored service errors as `None`s
					.try_filter_map(|x| futures::future::ok(x))
					// we use `select_with_weak` here, instead of `select`, to close the stream
					// as soon as the ipc pipe is closed
					.select_with_weak(receiver.map(Ok));

				responses.forward(writer).then(move |_| {
					trace!(target: "ipc", "Peer: service finished");
					if let Some(stats) = session_stats.as_ref() {
						stats.close_session(session_id)
					}

					async { Ok(()) }
				})
			});
			start_signal
				.send(Ok(()))
				.expect("Cannot fail since receiver never dropped before receiving");
			let stop = stop_receiver.map_err(|_| std::io::ErrorKind::Interrupted);
			let stop = Box::pin(stop);

			let server = server.try_buffer_unordered(1024).for_each(|_| async {});

			let result = futures::future::select(Box::pin(server), stop).await;
			// We drop the server first to prevent a situation where main thread terminates
			// before the server is properly dropped (see #504 for more details)
			drop(result);
			let _ = wait_signal.send(());
		};

		use futures::FutureExt;
		let fut = Box::pin(fut.map(drop));
		executor.executor().spawn(fut);

		let handle = InnerHandles {
			executor: Some(executor),
			stop: Some(stop_signal),
			path: path.to_owned(),
		};

		use futures::TryFutureExt;
		match start_receiver.recv().expect("Message should always be sent") {
			Ok(()) => Ok(Server {
				handles: Arc::new(Mutex::new(handle)),
				wait_handle: Some(wait_receiver),
			}),
			Err(e) => Err(e),
		}
	}
}

/// IPC Server handle
#[derive(Debug)]
pub struct Server {
	handles: Arc<Mutex<InnerHandles>>,
	wait_handle: Option<std::sync::mpsc::Receiver<()>>,
}

impl Server {
	/// Closes the server (waits for finish)
	pub fn close(self) {
		self.handles.lock().close();
	}

	/// Creates a close handle that can be used to stop the server remotely
	pub fn close_handle(&self) -> CloseHandle {
		CloseHandle {
			inner: self.handles.clone(),
		}
	}

	/// Wait for the server to finish
	pub fn wait(mut self) {
		if let Some(wait_receiver) = self.wait_handle.take() {
			let _ = wait_receiver.recv();
		}
	}
}

#[derive(Debug)]
struct InnerHandles {
	executor: Option<reactor::Executor>,
	stop: Option<oneshot::Sender<()>>,
	path: String,
}

impl InnerHandles {
	pub fn close(&mut self) {
		let _ = self.stop.take().map(|stop| stop.send(()));
		if let Some(executor) = self.executor.take() {
			executor.close()
		}
		let _ = ::std::fs::remove_file(&self.path); // ignore error, file could have been gone somewhere
	}
}

impl Drop for InnerHandles {
	fn drop(&mut self) {
		self.close();
	}
}
/// `CloseHandle` allows one to stop an `IpcServer` remotely.
#[derive(Clone)]
pub struct CloseHandle {
	inner: Arc<Mutex<InnerHandles>>,
}

impl CloseHandle {
	/// `close` closes the corresponding `IpcServer` instance.
	pub fn close(self) {
		self.inner.lock().close();
	}
}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
	use super::*;

	use jsonrpc_core::Value;
	use std::os::unix::net::UnixStream;
	use std::thread;
	use std::time::{self, Duration};

	fn server_builder() -> ServerBuilder {
		let mut io = MetaIoHandler::<()>::default();
		io.add_sync_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
		ServerBuilder::new(io)
	}

	fn run(path: &str) -> Server {
		let builder = server_builder();
		let server = builder.start(path).expect("Server must run with no issues");
		server
	}

	fn dummy_request_str(path: &str, data: &str) -> String {
		use futures::SinkExt;

		let reply = async move {
			use tokio::net::UnixStream;

			let stream: UnixStream = UnixStream::connect(path).await?;
			let codec = codecs::StreamCodec::stream_incoming();
			let mut stream = tokio_util::codec::Decoder::framed(codec, stream);
			stream.send(data.to_owned()).await?;
			let (reply, _) = stream.into_future().await;

			reply.expect("there should be one reply")
		};

		let mut rt = tokio::runtime::Runtime::new().unwrap();
		rt.block_on(reply).expect("wait for reply")
	}

	#[test]
	fn start() {
		crate::logger::init_log();

		let mut io = MetaIoHandler::<()>::default();
		io.add_sync_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
		let server = ServerBuilder::new(io);

		let _server = server
			.start("/tmp/test-ipc-20000")
			.expect("Server must run with no issues");
	}

	#[test]
	fn connect() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-30000";
		let _server = run(path);

		UnixStream::connect(path).expect("Socket should connect");
	}

	#[test]
	fn request() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-40000";
		let server = run(path);
		let (stop_signal, stop_receiver) = std::sync::mpsc::channel();

		let t = thread::spawn(move || {
			let result = dummy_request_str(
				path,
				"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}",
			);
			stop_signal.send(result).unwrap();
		});
		t.join().unwrap();

		let result = stop_receiver.recv().unwrap();

		assert_eq!(
			result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
			"Response does not exactly match the expected response",
		);
		server.close();
	}

	#[test]
	fn req_parallel() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-45000";
		let server = run(path);
		let (stop_signal, stop_receiver) = futures::channel::mpsc::channel(400);

		let mut handles = Vec::new();
		for _ in 0..4 {
			let path = path.clone();
			let mut stop_signal = stop_signal.clone();
			handles.push(thread::spawn(move || {
				for _ in 0..100 {
					let result = dummy_request_str(
						&path,
						"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}",
					);
					stop_signal.try_send(result).unwrap();
				}
			}));
		}

		for handle in handles.drain(..) {
			handle.join().unwrap();
		}

		thread::spawn(move || {
			let fut = stop_receiver
				.map(|result| {
					assert_eq!(
						result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
						"Response does not exactly match the expected response",
					);
				})
				.take(400)
				.for_each(|_| async {});
			futures::executor::block_on(fut);
		})
		.join()
		.unwrap();
		server.close();
	}

	#[test]
	fn close() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-50000";
		let server = run(path);
		server.close();

		assert!(
			::std::fs::metadata(path).is_err(),
			"There should be no socket file left"
		);
		assert!(
			UnixStream::connect(path).is_err(),
			"Connection to the closed socket should fail"
		);
	}

	fn huge_response_test_str() -> String {
		let mut result = String::from("begin_hello");
		result.push_str("begin_hello");
		for _ in 0..16384 {
			result.push(' ');
		}
		result.push_str("end_hello");
		result
	}

	fn huge_response_test_json() -> String {
		let mut result = String::from("{\"jsonrpc\":\"2.0\",\"result\":\"");
		result.push_str(&huge_response_test_str());
		result.push_str("\",\"id\":1}");

		result
	}

	#[test]
	fn test_huge_response() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-60000";

		let mut io = MetaIoHandler::<()>::default();
		io.add_sync_method("say_huge_hello", |_params| Ok(Value::String(huge_response_test_str())));
		let builder = ServerBuilder::new(io);

		let server = builder.start(path).expect("Server must run with no issues");
		let (stop_signal, stop_receiver) = oneshot::channel();

		let t = thread::spawn(move || {
			let result = dummy_request_str(
				&path,
				"{\"jsonrpc\": \"2.0\", \"method\": \"say_huge_hello\", \"params\": [], \"id\": 1}",
			);

			stop_signal.send(result).unwrap();
		});
		t.join().unwrap();

		thread::spawn(move || {
			futures::executor::block_on(async move {
				let result = stop_receiver.await.unwrap();
				assert_eq!(
					result,
					huge_response_test_json(),
					"Response does not exactly match the expected response",
				);
				server.close();
			});
		})
		.join()
		.unwrap();
	}

	#[test]
	fn test_session_end() {
		struct SessionEndMeta {
			drop_signal: Option<oneshot::Sender<()>>,
		}

		impl Drop for SessionEndMeta {
			fn drop(&mut self) {
				trace!(target: "ipc", "Dropping session meta");
				self.drop_signal.take().unwrap().send(()).unwrap()
			}
		}

		struct SessionEndExtractor {
			drop_receivers: Arc<Mutex<futures::channel::mpsc::Sender<oneshot::Receiver<()>>>>,
		}

		impl MetaExtractor<Arc<SessionEndMeta>> for SessionEndExtractor {
			fn extract(&self, _context: &RequestContext) -> Arc<SessionEndMeta> {
				let (signal, receiver) = oneshot::channel();
				self.drop_receivers.lock().try_send(receiver).unwrap();
				let meta = SessionEndMeta {
					drop_signal: Some(signal),
				};
				Arc::new(meta)
			}
		}

		crate::logger::init_log();
		let path = "/tmp/test-ipc-30009";
		let (signal, receiver) = futures::channel::mpsc::channel(16);
		let session_metadata_extractor = SessionEndExtractor {
			drop_receivers: Arc::new(Mutex::new(signal)),
		};

		let io = MetaIoHandler::<Arc<SessionEndMeta>>::default();
		let builder = ServerBuilder::with_meta_extractor(io, session_metadata_extractor);
		let server = builder.start(path).expect("Server must run with no issues");
		{
			let _ = UnixStream::connect(path).expect("Socket should connect");
		}

		thread::spawn(move || {
			futures::executor::block_on(async move {
				let (drop_receiver, ..) = receiver.into_future().await;
				drop_receiver.unwrap().await.unwrap();
			});
		})
		.join()
		.unwrap();
		server.close();
	}

	#[test]
	fn close_handle() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-90000";
		let server = run(path);
		let handle = server.close_handle();
		handle.close();
		assert!(
			UnixStream::connect(path).is_err(),
			"Connection to the closed socket should fail"
		);
	}

	#[test]
	fn close_when_waiting() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-70000";
		let server = run(path);
		let close_handle = server.close_handle();
		let (tx, rx) = oneshot::channel();

		thread::spawn(move || {
			thread::sleep(time::Duration::from_millis(100));
			close_handle.close();
		});
		thread::spawn(move || {
			server.wait();
			tx.send(true).expect("failed to report that the server has stopped");
		});

		let mut rt = tokio::runtime::Runtime::new().unwrap();
		rt.block_on(async move {
			let timeout = tokio::time::delay_for(Duration::from_millis(500));

			match futures::future::select(rx, timeout).await {
				futures::future::Either::Left((result, _)) => {
					assert!(result.is_ok(), "Rx failed");
					assert_eq!(result, Ok(true), "Wait timeout exceeded");
					assert!(
						UnixStream::connect(path).is_err(),
						"Connection to the closed socket should fail"
					);
					Ok(())
				}
				futures::future::Either::Right(_) => Err("timed out"),
			}
		})
		.unwrap();
	}

	#[test]
	fn runs_with_security_attributes() {
		let path = "/tmp/test-ipc-9001";
		let io = MetaIoHandler::<Arc<()>>::default();
		ServerBuilder::with_meta_extractor(io, NoopExtractor)
			.set_security_attributes(SecurityAttributes::empty())
			.start(path)
			.expect("Server must run with no issues");
	}
}
