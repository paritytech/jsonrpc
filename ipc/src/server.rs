use std;
use std::sync::Arc;

use tokio_service::{self, Service as TokioService};
use jsonrpc::futures::{future, Future, Stream, Sink};
use jsonrpc::futures::sync::oneshot;
use jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use jsonrpc::futures::BoxFuture;
use server_utils::tokio_core::io::Io;
use server_utils::tokio_core::reactor::Remote;
use server_utils::reactor;

use meta::{MetaExtractor, NoopExtractor, RequestContext};

/// IPC server session
pub struct Service<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
	/// Create new IPC server session with given handler and metadata.
	pub fn new(handler: Arc<MetaIoHandler<M, S>>, meta: M) -> Self {
		Service { handler: handler, meta: meta }
	}
}

impl<M: Metadata, S: Middleware<M>> tokio_service::Service for Service<M, S> {
	type Request = String;
	type Response = Option<String>;

	type Error = ();

	type Future = BoxFuture<Self::Response, Self::Error>;

	fn call(&self, req: Self::Request) -> Self::Future {
		trace!(target: "ipc", "Received request: {}", req);
		self.handler.handle_request(&req, self.meta.clone())
	}
}

/// IPC server builder
pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	remote: reactor::UninitializedRemote,
}

impl<M: Metadata, S: Middleware<M>> ServerBuilder<M, S> {
	///
	pub fn new<T>(io_handler: T) -> ServerBuilder<M, S> where
		T: Into<MetaIoHandler<M, S>>,
	{
		ServerBuilder {
			handler: Arc::new(io_handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			remote: reactor::UninitializedRemote::Unspawned,
		}
	}

	/// Sets shared different event loop remote.
	pub fn event_loop_remote(mut self, remote: Remote) -> Self {
		self.remote = reactor::UninitializedRemote::Shared(remote);
		self
	}

	/// Sets session metadata extractor.
	pub fn session_metadata_extractor<X>(mut self, meta_extractor: X) -> Self where
		X: MetaExtractor<M>,
	{
		self.meta_extractor = Arc::new(meta_extractor);
		self
	}

	/// Run server (in a separate thread)
	pub fn start(self, path: &str) -> std::io::Result<Server> {
		let remote = self.remote.initialize()?;
		let rpc_handler = self.handler.clone();
		let endpoint_addr = path.to_owned();
		let meta_extractor = self.meta_extractor.clone();
		let (stop_signal, stop_receiver) = oneshot::channel();
		let (start_signal, start_receiver) = oneshot::channel();

		remote.remote().spawn(move |handle| {
			use parity_tokio_ipc::Endpoint;
			use stream_codec::StreamCodec;

			if cfg!(unix) {
				// warn about existing file and remove it
				if ::std::fs::remove_file(&endpoint_addr).is_ok() {
					warn!("Removed existing file '{}'.", &endpoint_addr);
				} 
			}

			let listener = match Endpoint::new(endpoint_addr, handle) {
				Ok(l) => l,
				Err(e) => {
					start_signal.complete(Err(e));
					return future::ok(()).boxed();
				}
			};

			start_signal.complete(Ok(()));
			let remote = handle.remote().clone();
			let connections = listener.incoming();

			let server = connections.for_each(move |(io_stream, remote_id)| {
				trace!("Accepted incoming IPC connection");

				let meta = meta_extractor.extract(&RequestContext { endpoint_addr: &remote_id });
				let service = Service::new(rpc_handler.clone(), meta);
				let (writer, reader) = io_stream.framed(StreamCodec).split();
				let responses = reader.and_then(
					move |req| service.call(req).then(|response| match response {
						Err(e) => {
							warn!(target: "ipc", "Error while processing request: {:?}", e);
							future::ok(None)
						},
						Ok(None) => {
							future::ok(None)
						},
						Ok(Some(response_data)) => {
							trace!(target: "ipc", "Sent response: {}", &response_data);
							future::ok(Some(response_data))
						}
					})
				)
				.filter_map(|x| x);


				let writer = writer.send_all(responses).then(move |_| {
					trace!(target: "ipc", "Peer: service finished");
					Ok(())
				});

				remote.spawn(|_| writer);

				Ok(())
			});

			let stop = stop_receiver.map_err(|_| std::io::ErrorKind::Interrupted.into());
			server.select(stop).map(|_| ()).map_err(|_| ()).boxed()
		});

		match start_receiver.wait().expect("Message should always be sent") {
			Ok(()) => Ok(Server { path: path.to_owned(), remote: Some(remote), stop: Some(stop_signal) }),
			Err(e) => Err(e)
		}
	}
}


/// IPC Server handle
pub struct Server {
	path: String,
	remote: Option<reactor::Remote>,
	stop: Option<oneshot::Sender<()>>,
}

impl Server {
	/// Closes the server (waits for finish)
	pub fn close(mut self) {
		self.stop.take().unwrap().complete(());
		self.remote.take().unwrap().close();
		self.clear_file();
	}

	/// Wait for the server to finish
	pub fn wait(mut self) {
		self.remote.take().unwrap().wait();
	}

	/// Remove socket file
	fn clear_file(&self) {
		::std::fs::remove_file(&self.path).unwrap_or_else(|_| {}); // ignore error, file could have been gone somewhere
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.stop.take().map(|stop| stop.complete(()));
		self.remote.take().map(|remote| remote.close());
		self.clear_file();
	}
}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
	extern crate tokio_uds;

	use std::thread;
	use super::{ServerBuilder, Server};
	use jsonrpc::{MetaIoHandler, Value};
	use jsonrpc::futures::{Future, future};
	use self::tokio_uds::UnixStream;
	use server_utils::tokio_core::reactor::Core;
	use server_utils::tokio_core::io;

	fn server_builder() -> ServerBuilder {
		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_hello", |_params| {
			Ok(Value::String("hello".to_string()))
		});
		ServerBuilder::new(io)
	}

	fn run(path: &str) -> Server {
		let builder = server_builder();
		let server = builder.start(path).expect("Server must run with no issues");
		thread::sleep(::std::time::Duration::from_millis(50));
		server
	}

	fn dummy_request(path: &str, data: &[u8]) -> Vec<u8> {
		let mut core = Core::new().expect("Tokio Core should be created with no errors");
		let mut buffer = vec![0u8; 1024];

		let stream = UnixStream::connect(path, &core.handle()).expect("Should have been connected to the server");
		let reqrep = io::write_all(stream, data)
			.and_then(|(stream, _)| {
				io::read(stream, &mut buffer)
			})
			.and_then(|(_, read_buf, len)| {
				future::ok(read_buf[0..len].to_vec())
			});
		let result = core.run(reqrep).expect("Core should run with no errors");

		result
	}

	fn dummy_request_str(path: &str, data: &[u8]) -> String {
		String::from_utf8(dummy_request(path, data)).expect("String should be utf-8")
	}

	#[test]
	fn start() {
		::logger::init_log();

		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_hello", |_params| {
			Ok(Value::String("hello".to_string()))
		});
		let server = ServerBuilder::new(io);

		let _server = server.start("/tmp/test-ipc-20000")
			.expect("Server must run with no issues");
	}

	#[test]
	fn connect() {
		::logger::init_log();
		let path = "/tmp/test-ipc-30000";
		let _server = run(path);

		let core = Core::new().expect("Tokio Core should be created with no errors");
		UnixStream::connect(path, &core.handle()).expect("Socket should connect");
	}

	#[test]
	fn request() {
		::logger::init_log();
		let path = "/tmp/test-ipc-40000";
		let _server = run(path);

		let result = dummy_request_str(
			path,
			b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n",
			);

		assert_eq!(
			result,
			"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
			"Response does not exactly much the expected response",
			);
	}

	#[test]
	fn close() {
		::logger::init_log();
		let path = "/tmp/test-ipc-50000";
		let server = run(path);
		server.close();

		assert!(::std::fs::metadata(path).is_err(), "There should be no socket file left");
		let core = Core::new().expect("Tokio Core should be created with no errors");
		assert!(UnixStream::connect(path, &core.handle()).is_err(), "Connection to the closed socket should fail");
	}
}
