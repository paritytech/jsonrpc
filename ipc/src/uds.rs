extern crate tokio_uds;
extern crate tokio_service;

pub extern crate jsonrpc_core as jsonrpc;
extern crate jsonrpc_server_utils as server_utils;

use std;
use std::sync::{Arc, Mutex};

use self::tokio_service::Service as TokioService;
use self::jsonrpc::futures::{future, Future, Stream, Sink};
use self::jsonrpc::futures::sync::{mpsc, oneshot};
use self::jsonrpc::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use self::jsonrpc::futures::BoxFuture;
use self::server_utils::tokio_core::io::Io;
use self::server_utils::{reactor, tokio_core};

use meta::{MetaExtractor, NoopExtractor};

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

/// IPC Server handle
pub struct Server {
	remote: Option<reactor::Remote>,
	path: String,
}

impl Drop for Server {
	fn drop(&mut self) {
		::std::fs::remove_file(&self.path).unwrap_or_else(|_| {}); // ignoer error - server could have never been started
	}
}

impl Server {
	fn with_remote(remote: reactor::Remote, path: String) -> Self {
		Server { remote: Some(remote), path: path }
	}
}

impl<M: Metadata, S: Middleware<M> + Send + Sync + 'static> ServerBuilder<M, S> {
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

		remote.remote().spawn(move |handle| {
			use self::tokio_uds::UnixListener;
			use stream_codec::StreamCodec;

			let listener = match UnixListener::bind(&endpoint_addr, handle) {
				Ok(l) => l,
				Err(e) => {
					error!("Error binding domain socket: {}", e);
					return future::err(e).map_err(|_| ()).boxed();
				}
			};

			let remote = handle.remote().clone();
			let connections = listener.incoming();

			let server = connections.for_each(move |(unix_stream, client_addr)| {

				trace!("Accepted incoming UDS connection: {:?}", client_addr);

				// TODO: meta
				let meta = Default::default();

				let service = Service::new(rpc_handler.clone(), meta);
				let (writer, reader) = unix_stream.framed(StreamCodec).split();
				let responses = reader.and_then(
					move |req| service.call(req).then(|response| match response {
						Err(e) => {
							warn!(target: "uds", "Error while processing request: {:?}", e);
							future::ok(String::new())
						},
						Ok(None) => {
							trace!(target: "uds", "JSON RPC request produced no response");
							future::ok(String::new())
						},
						Ok(Some(response_data)) => {
							trace!(target: "uds", "Sent response: {}", &response_data);
							future::ok(response_data)
						}
					})
				);	

				let writer = writer.send_all(responses).then(move |_| {
					trace!(target: "uds", "Peer {:?}: service finished", client_addr);
					Ok(())
				});

				remote.spawn(|_| writer);

				Ok(())
			});

			server.map_err(|e| { error!("Error starting: {:?}", e); }).boxed()
		});


		Ok(Server::with_remote(remote, path.to_owned()))			
	}	
}

#[cfg(test)]
mod tests {
	use std::thread;
	use super::ServerBuilder;
	use super::jsonrpc::{MetaIoHandler, Value, Metadata};
	use super::jsonrpc::futures::{Future, future};
	use super::tokio_uds::UnixStream;
	use super::tokio_core::reactor::Core;
	use super::tokio_core::io;

	fn server_builder() -> ServerBuilder {
		let mut io = MetaIoHandler::<()>::default();
		io.add_method("say_hello", |_params| {
			Ok(Value::String("hello".to_string()))
		});
		ServerBuilder::new(io)
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
		let builder = server_builder();
		let _server = builder.start(path).expect("Server must run with no issues");
		thread::sleep(::std::time::Duration::from_millis(50));

		let mut core = Core::new().expect("Tokio Core should be created with no errors");
		UnixStream::connect(path, &core.handle()).expect("Socket should connect");
	}

	#[test]
	fn request() {
		::logger::init_log();
		let path = "/tmp/test-ipc-40000";
		let server = server_builder();
		let _server = server.start(path).expect("Server must run with no issues");
		thread::sleep(::std::time::Duration::from_millis(50));

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
}