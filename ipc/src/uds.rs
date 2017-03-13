extern crate tokio_uds;
extern crate tokio_service;

pub extern crate jsonrpc_core as jsonrpc;
pub extern crate jsonrpc_server_utils as server_utils;

use std;
use std::sync::Arc;

use self::tokio_service::Service as TokioService;
use self::jsonrpc::futures::{future, Future, Stream, Sink};
use self::jsonrpc::futures::sync::oneshot;
use self::jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use self::jsonrpc::futures::BoxFuture;
use self::server_utils::tokio_core::io::Io;
use self::server_utils::reactor;

use meta::{MetaExtractor, NoopExtractor, RequestContext};

pub struct Service<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta: M,
}

impl<M: Metadata, S: Middleware<M>> Service<M, S> {
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
		trace!(target: "tcp", "Accepted request: {}", req);
		self.handler.handle_request(&req, self.meta.clone())
	}
}

pub struct ServerBuilder<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	remote: reactor::UnitializedRemote,
}

impl<M: Metadata, S: Middleware<M>> ServerBuilder<M, S> {
	pub fn new<T>(io_handler: T) -> ServerBuilder<M, S> where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_remote(
			io_handler,
			reactor::UnitializedRemote::Unspawned,
		)
	}

	pub fn with_remote<T>(
		io_handler: T,
		remote: reactor::UnitializedRemote,
	) -> ServerBuilder<M, S>
		where T: Into<MetaIoHandler<M, S>>,
	{
		ServerBuilder {
			handler: Arc::new(io_handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			remote: remote,
		}
	}	

	/// Run server (in separate thread)
	pub fn start(self, path: &str) -> std::io::Result<Server> {
		let remote = self.remote.initialize()?;
		let rpc_handler = self.handler.clone();		
		let endpoint_addr = path.to_owned();
		let meta_extractor = self.meta_extractor.clone();
		let (stop_signal, stop_receiver) = oneshot::channel();
		let (start_signal, start_receiver) = oneshot::channel();

		remote.remote().spawn(move |handle| {
			use self::tokio_uds::UnixListener;
			use stream_codec::StreamCodec;

			let listener = match UnixListener::bind(&endpoint_addr, handle) {
				Ok(l) => l,
				Err(e) => {
					start_signal.complete(Err(e));
					return future::ok(()).boxed();
				}
			};

			start_signal.complete(Ok(()));
			let remote = handle.remote().clone();
			let connections = listener.incoming();

			let server = connections.for_each(move |(unix_stream, client_addr)| {
				trace!("Accepted incoming UDS connection: {:?}", client_addr);

				let meta = meta_extractor.extract(&RequestContext { endpoint_addr: &client_addr });
				let service = Service::new(rpc_handler.clone(), meta);
				let (writer, reader) = unix_stream.framed(StreamCodec).split();
				let responses = reader.and_then(
					move |req| service.call(req).then(|response| match response {
						Err(e) => {
							warn!(target: "uds", "Error while processing request: {:?}", e);
							future::ok(None)
						},
						Ok(None) => {
							future::ok(None)
						},
						Ok(Some(response_data)) => {
							trace!(target: "uds", "Sent response: {}", &response_data);
							future::ok(Some(response_data))
						}
					})
				)
				.filter_map(|x| x);


				let writer = writer.send_all(responses).then(move |_| {
					trace!(target: "uds", "Peer {:?}: service finished", client_addr);
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

pub fn server<I, M: Metadata, S: Middleware<M>>(
	io: I, 
	path: &str
) -> std::io::Result<Server>
	where I: Into<MetaIoHandler<M, S>>
{
	ServerBuilder::new(io).start(path)
}

#[cfg(test)]
mod tests {
	use std::thread;
	use super::{ServerBuilder, Server};
	use super::jsonrpc::{MetaIoHandler, Value};
	use super::jsonrpc::futures::{Future, future};
	use super::tokio_uds::UnixStream;
	use super::server_utils::tokio_core::reactor::Core;
	use super::server_utils::tokio_core::io;

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