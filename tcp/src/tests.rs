use std::cell::RefCell;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use jsonrpc::{MetaIoHandler, Value, Metadata};
use jsonrpc::futures::{Future, future};

use server_utils::tokio_io::io;
use server_utils::tokio_core::net::TcpStream;
use server_utils::tokio_core::reactor::{Core, Timeout};

use parking_lot::Mutex;

use ServerBuilder;
use MetaExtractor;
use RequestContext;

fn casual_server() -> ServerBuilder {
	let mut io = MetaIoHandler::<()>::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_string()))
	});
	ServerBuilder::new(io)
}

#[test]
fn doc_test() {
	::logger::init_log();

	let mut io = MetaIoHandler::<()>::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_string()))
	});
	let server = ServerBuilder::new(io);

	server.start(&SocketAddr::from_str("0.0.0.0:17770").unwrap())
		.expect("Server must run with no issues")
		.close()
}

#[test]
fn doc_test_connect() {
	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17775".parse().unwrap();
	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let mut core = Core::new().expect("Tokio Core should be created with no errors");
	let stream = TcpStream::connect(&addr, &core.handle());
	let result = core.run(stream);

	assert!(result.is_ok());
}

#[test]
fn disconnect() {
	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17777".parse().unwrap();
	let server = casual_server();
	let dispatcher = server.dispatcher();
	let _server = server.start(&addr).expect("Server must run with no issues");

	{
		let mut core = Core::new().expect("Tokio Core should be created with no errors");
		let stream = TcpStream::connect(&addr, &core.handle())
			.and_then(|stream| future::ok(stream))
			.and_then(|stream| future::result(stream.shutdown(::std::net::Shutdown::Both)));
		core.run(stream).expect("tcp/ip session should finalize with no errors in disconnect test");
	}

	::std::thread::sleep(::std::time::Duration::from_millis(50));

	assert_eq!(0, dispatcher.peer_count());
}

fn dummy_request(addr: &SocketAddr, data: &[u8]) -> Vec<u8> {
	let mut core = Core::new().expect("Tokio Core should be created with no errors");
	let mut buffer = vec![0u8; 1024];

	let stream = TcpStream::connect(addr, &core.handle())
		.and_then(|stream| {
			io::write_all(stream, data)
		})
	.and_then(|(stream, _)| {
		io::read(stream, &mut buffer)
	})
	.and_then(|(_, read_buf, len)| {
		future::ok(read_buf[0..len].to_vec())
	});
	let result = core.run(stream).expect("Core should run with no errors");

	result
}

fn dummy_request_str(addr: &SocketAddr, data: &[u8]) -> String {
	String::from_utf8(dummy_request(addr, data)).expect("String should be utf-8")
}

#[test]
fn doc_test_handle() {
	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17780".parse().unwrap();
	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let result = dummy_request_str(
		&addr,
		b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n",
		);

	assert_eq!(
		result,
		"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
		"Response does not exactly much the expected response",
		);
}

#[test]
fn req_parallel() {
	use std::thread;

	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17782".parse().unwrap();
	let server = casual_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let mut handles = Vec::new();
	for _ in 0..6 {
		let addr = addr.clone();
		handles.push(
			thread::spawn(move || {
				for _ in 0..100 {
					let result = dummy_request_str(
						&addr,
						b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n",
						);

					assert_eq!(
						result,
						"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
						"Response does not exactly much the expected response",
						);
				}
			})
		);
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
		SocketMetadata { addr: "0.0.0.0:0".parse().unwrap() }
	}
}

impl SocketMetadata {
	pub fn addr(&self) -> &SocketAddr {
		&self.addr
	}
}

impl Metadata for SocketMetadata { }

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
		future::ok(Value::String(format!("hello, {}", meta.addr())))
	});
	ServerBuilder::new(io).session_meta_extractor(PeerMetaExtractor)
}

#[test]
fn peer_meta() {
	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17785".parse().unwrap();
	let server = meta_server();
	let _server = server.start(&addr).expect("Server must run with no issues");

	let result = dummy_request_str(
		&addr,
		b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n"
		);

	// contains random port, so just smoky comparing response length
	assert_eq!(
		59,
		result.len()
		);
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
	::logger::init_log();
	let addr: SocketAddr = "127.0.0.1:17790".parse().unwrap();
	let mut io = MetaIoHandler::<SocketMetadata>::default();
	io.add_method_with_meta("say_hello", |_params, _: SocketMetadata| {
		future::ok(Value::String("hello".to_owned()))
	});
	let extractor = PeerListMetaExtractor::default();
	let peer_list = extractor.peers.clone();
	let server = ServerBuilder::new(io)
		.session_meta_extractor(extractor);
	let dispatcher = server.dispatcher();

	let _server = server.start(&addr).expect("Server must run with no issues");

	let mut core = Core::new().expect("Tokio Core should be created with no errors");
	let timeout = Timeout::new(::std::time::Duration::from_millis(100), &core.handle())
		.expect("There should be a timeout produced in message test");
	let mut buffer = vec![0u8; 1024];
	let mut buffer2 = vec![0u8; 1024];
	let executed_dispatch = RefCell::new(false);
	let executed_request = RefCell::new(false);

	// CLIENT RUN
	let stream = TcpStream::connect(&addr, &core.handle())
		.and_then(|stream| {
			future::ok(stream).join(timeout)
		})
	.and_then(|stream| {
		let peer_addr = peer_list.lock()[0].clone();
		dispatcher.push_message(
			&peer_addr,
			"ping".to_owned(),
			).expect("Should be sent with no errors");
		trace!(target: "tcp", "Dispatched message for {}", peer_addr);
		future::ok(stream)
	})
	.and_then(|(stream, _)| {
		io::read(stream, &mut buffer)
	})
	.and_then(|(stream, read_buf, len)| {
		trace!(target: "tcp", "Read ping message");
		let ping_signal = read_buf[0..len].to_vec();

		assert_eq!(
			"ping\n",
			String::from_utf8(ping_signal).expect("String should be utf-8"),
			"Sent request does not match received by the peer",
			);
		// ensure tat the above assert was actually triggered
		*executed_dispatch.borrow_mut() = true;

		future::ok(stream)
	})
	.and_then(|stream| {
		// make request AFTER message dispatches
		let data = b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n";
		io::write_all(stream, &data[..])
	})
	.and_then(|(stream, _)| {
		io::read(stream, &mut buffer2)
	})
	.and_then(|(_, read_buf, len)| {
		trace!(target: "tcp", "Read response message");
		let response_signal = read_buf[0..len].to_vec();
		assert_eq!(
			"{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
			String::from_utf8(response_signal).expect("String should be utf-8"),
			"Response does not match the expected handling",
			);
		*executed_request.borrow_mut() = true;

		future::ok(())
	});

	core.run(stream).expect("Should be the payload in message test");
	assert!(*executed_dispatch.borrow_mut());
	assert!(*executed_request.borrow_mut());
}
