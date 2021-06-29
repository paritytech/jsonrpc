use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use jsonrpc_core::{MetaIoHandler, Metadata, Value};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use crate::futures;
use crate::server_utils::tokio::{self, net::TcpStream};

use parking_lot::Mutex;

use crate::MetaExtractor;
use crate::RequestContext;
use crate::ServerBuilder;

fn casual_server() -> ServerBuilder {
	let mut io = MetaIoHandler::<()>::default();
	io.add_sync_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
	ServerBuilder::new(io)
}

fn run_future<O>(fut: impl std::future::Future<Output = O> + Send) -> O {
	let rt = tokio::runtime::Runtime::new().unwrap();
	rt.block_on(fut)
}

#[test]
fn doc_test() {
	crate::logger::init_log();

	let mut io = MetaIoHandler::<()>::default();
	io.add_sync_method("say_hello", |_params| Ok(Value::String("hello".to_string())));
	let server = ServerBuilder::new(io);

	server
		.start(&SocketAddr::from_str("0.0.0.0:17770").unwrap())
		.expect("Server must run with no issues")
		.close()
}

#[test]
fn doc_test_connect() {
	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17775".parse().unwrap();
	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	run_future(async move { TcpStream::connect(&addr).await }).expect("Server connection error");
}

#[test]
fn disconnect() {
	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17777".parse().unwrap();
	let server = casual_server();
	let dispatcher = server.dispatcher();
	let _server = server.start(&addr).expect("Server must run with no issues");

	run_future(async move {
		let mut stream = TcpStream::connect(&addr).await.unwrap();
		assert_eq!(stream.peer_addr().unwrap(), addr);
		stream.shutdown().await.unwrap();
	});

	::std::thread::sleep(::std::time::Duration::from_millis(50));

	assert_eq!(0, dispatcher.peer_count());
}

fn dummy_request(addr: &SocketAddr, data: Vec<u8>) -> Vec<u8> {
	let (ret_tx, ret_rx) = std::sync::mpsc::channel();

	let stream = async move {
		let mut stream = TcpStream::connect(addr).await?;
		stream.write_all(&data).await?;
		stream.shutdown().await?;
		let mut read_buf = vec![];
		let _ = stream.read_to_end(&mut read_buf).await;

		let _ = ret_tx.send(read_buf).map_err(|err| panic!("Unable to send {:?}", err));

		Ok::<(), Box<dyn std::error::Error>>(())
	};

	run_future(stream).unwrap();
	ret_rx.recv().expect("Unable to receive result")
}

fn dummy_request_str(addr: &SocketAddr, data: Vec<u8>) -> String {
	String::from_utf8(dummy_request(addr, data)).expect("String should be utf-8")
}

#[test]
fn doc_test_handle() {
	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17780".parse().unwrap();

	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let result = dummy_request_str(
		&addr,
		b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n"[..].to_owned(),
	);

	assert_eq!(
		result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
		"Response does not exactly much the expected response",
	);
}

#[test]
fn req_parallel() {
	use std::thread;

	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17782".parse().unwrap();
	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let mut handles = Vec::new();
	for _ in 0..6 {
		let addr = addr.clone();
		handles.push(thread::spawn(move || {
			for _ in 0..100 {
				let result = dummy_request_str(
					&addr,
					b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n"[..]
						.to_owned(),
				);

				assert_eq!(
					result, "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
					"Response does not exactly much the expected response",
				);
			}
		}));
	}

	for handle in handles.drain(..) {
		handle.join().unwrap();
	}
}

#[derive(Clone)]
pub struct SocketMetadata {
	addr: SocketAddr,
}

impl Default for SocketMetadata {
	fn default() -> Self {
		SocketMetadata {
			addr: "0.0.0.0:0".parse().unwrap(),
		}
	}
}

impl SocketMetadata {
	pub fn addr(&self) -> &SocketAddr {
		&self.addr
	}
}

impl Metadata for SocketMetadata {}

impl From<SocketAddr> for SocketMetadata {
	fn from(addr: SocketAddr) -> SocketMetadata {
		SocketMetadata { addr: addr }
	}
}

pub struct PeerMetaExtractor;

impl MetaExtractor<SocketMetadata> for PeerMetaExtractor {
	fn extract(&self, context: &RequestContext) -> SocketMetadata {
		context.peer_addr.into()
	}
}

fn meta_server() -> ServerBuilder<SocketMetadata> {
	let mut io = MetaIoHandler::<SocketMetadata>::default();
	io.add_method_with_meta("say_hello", |_params, meta: SocketMetadata| {
		jsonrpc_core::futures::future::ready(Ok(Value::String(format!("hello, {}", meta.addr()))))
	});
	ServerBuilder::new(io).session_meta_extractor(PeerMetaExtractor)
}

#[test]
fn peer_meta() {
	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17785".parse().unwrap();
	let server = meta_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let result = dummy_request_str(
		&addr,
		b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n"[..].to_owned(),
	);

	println!("{}", result);

	// contains random port, so just smoky comparing response length
	assert!(result.len() == 58 || result.len() == 59);
}

#[derive(Default)]
pub struct PeerListMetaExtractor {
	peers: Arc<Mutex<Vec<SocketAddr>>>,
}

impl MetaExtractor<SocketMetadata> for PeerListMetaExtractor {
	fn extract(&self, context: &RequestContext) -> SocketMetadata {
		trace!(target: "tcp", "extracting to peer list...");
		self.peers.lock().push(context.peer_addr.clone());
		context.peer_addr.into()
	}
}

#[test]
fn message() {
	// MASSIVE SETUP
	crate::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17790".parse().unwrap();
	let mut io = MetaIoHandler::<SocketMetadata>::default();
	io.add_method_with_meta("say_hello", |_params, _: SocketMetadata| {
		jsonrpc_core::futures::future::ready(Ok(Value::String("hello".to_owned())))
	});
	let extractor = PeerListMetaExtractor::default();
	let peer_list = extractor.peers.clone();
	let server = ServerBuilder::new(io).session_meta_extractor(extractor);
	let dispatcher = server.dispatcher();

	let _server = server.start(&addr).expect("Server must run with no issues");

	let message = "ping";
	let executed_dispatch = Arc::new(Mutex::new(false));
	let executed_request = Arc::new(Mutex::new(false));
	let executed_dispatch_move = executed_dispatch.clone();
	let executed_request_move = executed_request.clone();

	let client = async move {
		let stream = TcpStream::connect(&addr);
		let delay = tokio::time::sleep(Duration::from_millis(500));
		let (stream, _) = futures::join!(stream, delay);
		let mut stream = stream?;

		let peer_addr = peer_list.lock()[0].clone();
		dispatcher
			.push_message(&peer_addr, message.to_owned())
			.expect("Should be sent with no errors");
		trace!(target: "tcp", "Dispatched message for {}", peer_addr);

		// Read message plus newline appended by codec.
		let mut read_buf = vec![0u8; message.len() + 1];
		let _ = stream.read_exact(&mut read_buf).await?;

		trace!(target: "tcp", "Read ping message");
		let ping_signal = read_buf[..].to_vec();

		assert_eq!(
			format!("{}\n", message),
			String::from_utf8(ping_signal).expect("String should be utf-8"),
			"Sent request does not match received by the peer",
		);
		// ensure that the above assert was actually triggered
		*executed_dispatch_move.lock() = true;

		// make request AFTER message dispatches
		let data = b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n";
		stream.write_all(&data[..]).await?;

		stream.shutdown().await.unwrap();
		let mut read_buf = vec![];
		let _ = stream.read_to_end(&mut read_buf).await?;

		trace!(target: "tcp", "Read response message");
		let response_signal = read_buf[..].to_vec();
		assert_eq!(
			"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
			String::from_utf8(response_signal).expect("String should be utf-8"),
			"Response does not match the expected handling",
		);
		*executed_request_move.lock() = true;

		// delay
		Ok::<(), Box<dyn std::error::Error>>(())
	};

	run_future(client).unwrap();

	assert!(*executed_dispatch.lock());
	assert!(*executed_request.lock());
}
