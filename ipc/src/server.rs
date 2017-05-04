use std;
use std::sync::Arc;

use tokio_service::{self, Service as TokioService};
use jsonrpc::futures::{future, Future, Stream, Sink};
use jsonrpc::futures::sync::oneshot;
use jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use jsonrpc::futures::BoxFuture;

use server_utils::tokio_core::reactor::Remote;
use server_utils::tokio_io::AsyncRead;
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
					start_signal.send(Err(e)).expect("Cannot fail since receiver never dropped before receiving");
					return future::ok(()).boxed();
				}
			};

			start_signal.send(Ok(())).expect("Cannot fail since receiver never dropped before receiving");
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


				let writer = writer.send_all(responses).then(|_| {
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
		self.stop.take().map(|stop| stop.send(()));
		self.remote.take().unwrap().close();
		self.clear_file();
	}

	/// Wait for the server to finish
	pub fn wait(mut self) {
		self.remote.take().unwrap().wait();
	}

	/// Remove socket file
	fn clear_file(&self) {
		let _ = ::std::fs::remove_file(&self.path); // ignore error, file could have been gone somewhere
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.stop.take().map(|stop| stop.send(()));
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
	use jsonrpc::futures::{Future, future, Stream, Sink};
	use self::tokio_uds::UnixStream;
	use server_utils::tokio_core::reactor::Core;
	use server_utils::tokio_io::AsyncRead;
	use stream_codec::StreamCodec;

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

	fn dummy_request_str(path: &str, data: &str) -> String {
		let mut core = Core::new().expect("Tokio Core should be created with no errors");

		let stream = UnixStream::connect(path, &core.handle()).expect("Should have been connected to the server");
		let (writer, reader) = stream.framed(StreamCodec).split();
		let reply = writer
			.send(data.to_owned())
			.and_then(move |_| {
				reader.into_future().map_err(|(err, _)| err)
			})
			.and_then(|(reply, _)| {
				future::ok(reply.expect("there should be one reply"))
			});

		core.run(reply).unwrap()
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
			"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}",
			);

		assert_eq!(
			result,
			"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
			"Response does not exactly match the expected response",
			);
	}

	#[test]
	fn req_parallel() {
		use std::thread;

		::logger::init_log();
		let path = "/tmp/test-ipc-45000";
		let _server = run(path);

		let mut handles = Vec::new();
		for _ in 0..4 {
			let path = path.clone();
			handles.push(
				thread::spawn(move || {
					for _ in 0..100 {
						let result = dummy_request_str(
							&path,
							"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}",
							);

						assert_eq!(
							result,
							"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}",
							"Response does not exactly match the expected response",
							);				

						::std::thread::sleep(::std::time::Duration::from_millis(10));	
					}					
				})
			);
		}	

		for handle in handles.drain(..) {
			handle.join().unwrap();
		}
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

	fn huge_response_test_str() -> String {
		let mut result = String::from("begin_hello");
		result.push_str("begin_hello");
		for _ in 0..16384 { result.push(' '); }
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
		let path = "/tmp/test-ipc-60000";

		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_huge_hello", |_params| {
			Ok(Value::String(huge_response_test_str()))
		});
		let builder = ServerBuilder::new(io);

		let _server = builder.start(path).expect("Server must run with no issues");
		thread::sleep(::std::time::Duration::from_millis(50));

		let result = dummy_request_str(&path,
			"{\"jsonrpc\": \"2.0\", \"method\": \"say_huge_hello\", \"params\": [], \"id\": 1}",
		);

		assert_eq!(
			result,
			huge_response_test_json(),
			"Response does not exactly match the expected response",
			);

	}
	


}
