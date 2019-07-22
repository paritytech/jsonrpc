use std::sync::Arc;

use crate::jsonrpc::futures::sync::{mpsc, oneshot};
use crate::jsonrpc::futures::{future, Future, Sink, Stream};
use crate::jsonrpc::{middleware, FutureResult, MetaIoHandler, Metadata, Middleware};
use tokio_service::{self, Service as TokioService};

use crate::server_utils::{
	codecs, reactor, session,
	tokio::{self, reactor::Handle, runtime::TaskExecutor},
	tokio_codec::Framed,
};
use parking_lot::Mutex;

use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
use crate::select_with_weak::SelectWithWeakExt;
use parity_tokio_ipc::Endpoint;
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

impl<M: Metadata, S: Middleware<M>> tokio_service::Service for Service<M, S> {
	type Request = String;
	type Response = Option<String>;

	type Error = ();

	type Future = FutureResult<S::Future, S::CallFuture>;

	fn call(&self, req: Self::Request) -> Self::Future {
		trace!(target: "ipc", "Received request: {}", req);
		self.handler.handle_request(&req, self.meta.clone())
	}
}

/// IPC server builder
pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn MetaExtractor<M>>,
	session_stats: Option<Arc<dyn session::SessionStats>>,
	executor: reactor::UninitializedExecutor,
	reactor: Option<Handle>,
	incoming_separator: codecs::Separator,
	outgoing_separator: codecs::Separator,
	security_attributes: SecurityAttributes,
	client_buffer_size: usize,
}

impl<M: Metadata + Default, S: Middleware<M>> ServerBuilder<M, S> {
	/// Creates new IPC server build given the `IoHandler`.
	pub fn new<T>(io_handler: T) -> ServerBuilder<M, S>
	where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_meta_extractor(io_handler, NoopExtractor)
	}
}

impl<M: Metadata, S: Middleware<M>> ServerBuilder<M, S> {
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
			reactor: None,
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

	/// Sets different event loop I/O reactor.
	pub fn event_loop_reactor(mut self, reactor: Handle) -> Self {
		self.reactor = Some(reactor);
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
		let reactor = self.reactor;
		let rpc_handler = self.handler;
		let endpoint_addr = path.to_owned();
		let meta_extractor = self.meta_extractor;
		let session_stats = self.session_stats;
		let incoming_separator = self.incoming_separator;
		let outgoing_separator = self.outgoing_separator;
		let (stop_signal, stop_receiver) = oneshot::channel();
		let (start_signal, start_receiver) = oneshot::channel();
		let (wait_signal, wait_receiver) = oneshot::channel();
		let security_attributes = self.security_attributes;
		let client_buffer_size = self.client_buffer_size;

		executor.spawn(future::lazy(move || {
			let mut endpoint = Endpoint::new(endpoint_addr);
			endpoint.set_security_attributes(security_attributes);

			if cfg!(unix) {
				// warn about existing file and remove it
				if ::std::fs::remove_file(endpoint.path()).is_ok() {
					warn!("Removed existing file '{}'.", endpoint.path());
				}
			}

			// Make sure to construct Handle::default() inside Tokio runtime
			let reactor = reactor.unwrap_or_else(Handle::default);
			let connections = match endpoint.incoming(&reactor) {
				Ok(connections) => connections,
				Err(e) => {
					start_signal
						.send(Err(e))
						.expect("Cannot fail since receiver never dropped before receiving");
					return future::Either::A(future::ok(()));
				}
			};

			let mut id = 0u64;

			let server = connections.for_each(move |(io_stream, remote_id)| {
				id = id.wrapping_add(1);
				let session_id = id;
				let session_stats = session_stats.clone();
				trace!(target: "ipc", "Accepted incoming IPC connection: {}", session_id);
				if let Some(stats) = session_stats.as_ref() {
					stats.open_session(session_id)
				}

				let (sender, receiver) = mpsc::channel(16);
				let meta = meta_extractor.extract(&RequestContext {
					endpoint_addr: &remote_id,
					session_id,
					sender,
				});
				let service = Service::new(rpc_handler.clone(), meta);
				let (writer, reader) = Framed::new(
					io_stream,
					codecs::StreamCodec::new(incoming_separator.clone(), outgoing_separator.clone()),
				)
				.split();
				let responses = reader
					.map(move |req| {
						service
							.call(req)
							.then(|result| match result {
								Err(_) => future::ok(None),
								Ok(some_result) => future::ok(some_result),
							})
							.map_err(|_: ()| std::io::ErrorKind::Other.into())
					})
					.buffer_unordered(client_buffer_size)
					.filter_map(|x| x)
					// we use `select_with_weak` here, instead of `select`, to close the stream
					// as soon as the ipc pipe is closed
					.select_with_weak(receiver.map_err(|e| {
						warn!(target: "ipc", "Notification error: {:?}", e);
						std::io::ErrorKind::Other.into()
					}));

				let writer = writer.send_all(responses).then(move |_| {
					trace!(target: "ipc", "Peer: service finished");
					if let Some(stats) = session_stats.as_ref() {
						stats.close_session(session_id)
					}
					Ok(())
				});

				tokio::spawn(writer);

				Ok(())
			});
			start_signal
				.send(Ok(()))
				.expect("Cannot fail since receiver never dropped before receiving");

			let stop = stop_receiver.map_err(|_| std::io::ErrorKind::Interrupted.into());
			future::Either::B(
				server
					.select(stop)
					.map(|_| {
						let _ = wait_signal.send(());
					})
					.map_err(|_| ()),
			)
		}));

		let handle = InnerHandles {
			executor: Some(executor),
			stop: Some(stop_signal),
			path: path.to_owned(),
		};

		match start_receiver.wait().expect("Message should always be sent") {
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
	wait_handle: Option<oneshot::Receiver<()>>,
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
		self.wait_handle.take().map(|wait_receiver| wait_receiver.wait());
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
	use tokio_uds;

	use self::tokio_uds::UnixStream;
	use super::SecurityAttributes;
	use super::{Server, ServerBuilder};
	use crate::jsonrpc::futures::sync::{mpsc, oneshot};
	use crate::jsonrpc::futures::{future, Future, Sink, Stream};
	use crate::jsonrpc::{MetaIoHandler, Value};
	use crate::meta::{MetaExtractor, NoopExtractor, RequestContext};
	use crate::server_utils::codecs;
	use crate::server_utils::{
		tokio::{self, timer::Delay},
		tokio_codec::Decoder,
	};
	use parking_lot::Mutex;
	use std::sync::Arc;
	use std::thread;
	use std::time;
	use std::time::{Duration, Instant};

	fn server_builder() -> ServerBuilder {
		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
		ServerBuilder::new(io)
	}

	fn run(path: &str) -> Server {
		let builder = server_builder();
		let server = builder.start(path).expect("Server must run with no issues");
		server
	}

	fn dummy_request_str(path: &str, data: &str) -> String {
		let stream_future = UnixStream::connect(path);
		let reply = stream_future.and_then(|stream| {
			let stream = codecs::StreamCodec::stream_incoming().framed(stream);
			let reply = stream
				.send(data.to_owned())
				.and_then(move |stream| stream.into_future().map_err(|(err, _)| err))
				.and_then(|(reply, _)| future::ok(reply.expect("there should be one reply")));
			reply
		});

		reply.wait().expect("wait for reply")
	}

	#[test]
	fn start() {
		crate::logger::init_log();

		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
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

		UnixStream::connect(path).wait().expect("Socket should connect");
	}

	#[test]
	fn request() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-40000";
		let server = run(path);
		let (stop_signal, stop_receiver) = oneshot::channel();

		let t = thread::spawn(move || {
			let result = dummy_request_str(
				path,
				"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}",
			);
			stop_signal.send(result).unwrap();
		});
		t.join().unwrap();

		let _ = stop_receiver
			.map(move |result: String| {
				assert_eq!(
					result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
					"Response does not exactly match the expected response",
				);
				server.close();
			})
			.wait();
	}

	#[test]
	fn req_parallel() {
		crate::logger::init_log();
		let path = "/tmp/test-ipc-45000";
		let server = run(path);
		let (stop_signal, stop_receiver) = mpsc::channel(400);

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

		let _ = stop_receiver
			.map(|result| {
				assert_eq!(
					result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
					"Response does not exactly match the expected response",
				);
			})
			.take(400)
			.collect()
			.wait();
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
			UnixStream::connect(path).wait().is_err(),
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
		io.add_method("say_huge_hello", |_params| Ok(Value::String(huge_response_test_str())));
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

		let _ = stop_receiver
			.map(move |result: String| {
				assert_eq!(
					result,
					huge_response_test_json(),
					"Response does not exactly match the expected response",
				);
				server.close();
			})
			.wait();
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
			drop_receivers: Arc<Mutex<mpsc::Sender<oneshot::Receiver<()>>>>,
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
		let (signal, receiver) = mpsc::channel(16);
		let session_metadata_extractor = SessionEndExtractor {
			drop_receivers: Arc::new(Mutex::new(signal)),
		};

		let io = MetaIoHandler::<Arc<SessionEndMeta>>::default();
		let builder = ServerBuilder::with_meta_extractor(io, session_metadata_extractor);
		let server = builder.start(path).expect("Server must run with no issues");
		{
			let _ = UnixStream::connect(path).wait().expect("Socket should connect");
		}

		receiver
			.into_future()
			.map_err(|_| ())
			.and_then(|drop_receiver| drop_receiver.0.unwrap().map_err(|_| ()))
			.wait()
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
			UnixStream::connect(path).wait().is_err(),
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

		let delay = Delay::new(Instant::now() + Duration::from_millis(500))
			.map(|_| false)
			.map_err(|err| panic!("{:?}", err));

		let result_fut = rx.map_err(|_| ()).select(delay).then(move |result| match result {
			Ok((result, _)) => {
				assert_eq!(result, true, "Wait timeout exceeded");
				assert!(
					UnixStream::connect(path).wait().is_err(),
					"Connection to the closed socket should fail"
				);
				Ok(())
			}
			Err(_) => Err(()),
		});

		tokio::run(result_fut);
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
