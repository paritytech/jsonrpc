use std::io::{Read, Write};
use std::net::{TcpStream, Ipv4Addr};
use std::str::Lines;
use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use core;
use core::futures::Future;
use server_utils::hosts::DomainsValidation;
use ws;

use server::Server;
use server_builder::ServerBuilder;

struct Response {
	status: String,
	_headers: String,
	body: String,
}

impl Response {
	pub fn parse(response: String) -> Self {
		let mut lines = response.lines();
		let status = lines.next().unwrap().to_owned();
		let headers = Self::read_block(&mut lines);
		let body = Self::read_block(&mut lines);

		Response {
			status: status,
			_headers: headers,
			body: body,
		}
	}

	fn read_block(lines: &mut Lines) -> String {
		let mut block = String::new();
		loop {
			let line = lines.next();
			match line {
				Some("") | None => break,
				Some(v) => {
					block.push_str(v);
					block.push_str("\n");
				},
			}
		}
		block
	}
}

fn request(server: Server, request: &str) -> Response {
	let mut req = TcpStream::connect(server.addr()).unwrap();
	req.write_all(request.as_bytes()).unwrap();

	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();

	Response::parse(response)
}

fn serve(port: u16) -> (Server, Arc<AtomicUsize>) {
	use std::time::Duration;
	use core::futures::sync::oneshot;

	let pending = Arc::new(AtomicUsize::new(0));

	let counter = pending.clone();

	let mut io = core::IoHandler::default();
	io.add_method("hello", |_params: core::Params| Ok(core::Value::String("world".into())));
	io.add_method("hello_async", |_params: core::Params| {
		core::futures::finished(core::Value::String("world".into()))
	});
	io.add_method("record_pending", move |_params: core::Params| {
		counter.fetch_add(1, Ordering::SeqCst);
		let (send, recv) = oneshot::channel();
		::std::thread::spawn(move || {
			::std::thread::sleep(Duration::from_millis(500));

			let _ = send.send(());
		});

		let counter = counter.clone();
		recv.then(move |res| {
			if res.is_ok() {
				counter.fetch_sub(1, Ordering::SeqCst);
			}
			Ok(core::Value::String("complete".into()))
		})
	});

	let server = ServerBuilder::new(io)
		.allowed_origins(DomainsValidation::AllowOnly(vec!["https://parity.io".into()]))
		.allowed_hosts(DomainsValidation::AllowOnly(vec![format!("127.0.0.1:{}", port).into()]))
		.request_middleware(|req: &ws::Request| {
			if req.resource() == "/intercepted" {
				let mut res = ws::Response::new(200, "OK");
				res.set_body("Hello World!".to_owned());
				Some(res)
			} else {
				None
			}
		})
		.start(&format!("127.0.0.1:{}", port).parse().unwrap())
		.unwrap();

	(server, pending)
}

#[test]
fn should_disallow_not_whitelisted_origins() {
	// given
	let (server, _) = serve(30001);

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: 127.0.0.1:30001\r\n\
			Origin: http://test.io\r\n\
			Connection: close\r\n\
			\r\n\
			I shouldn't be read.\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
}

#[test]
fn should_disallow_not_whitelisted_hosts() {
	// given
	let (server, _) = serve(30002);

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: myhost:30002\r\n\
			Connection: close\r\n\
			\r\n\
			I shouldn't be read.\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
}

#[test]
fn should_allow_whitelisted_origins() {
	// given
	let (server, _) = serve(30003);

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: 127.0.0.1:30003\r\n\
			Origin: https://parity.io\r\n\
			Connection: close\r\n\
			\r\n\
			{}\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 400 Bad Request".to_owned());
}

#[test]
fn should_intercept_in_middleware() {
	// given
	let (server, _) = serve(30004);

	// when
	let response = request(server,
		"\
			GET /intercepted HTTP/1.1\r\n\
			Host: 127.0.0.1:30004\r\n\
			Origin: https://parity.io\r\n\
			Connection: close\r\n\
			\r\n\
			{}\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, "Hello World!\n".to_owned());
}

#[test]
fn drop_session_should_cancel() {
	use ws::{connect, CloseCode};

	// given
	let (_server, incomplete) = serve(30005);

	// when
	connect("ws://127.0.0.1:30005", |out| {
    	out.send(r#"{"jsonrpc":"2.0", "method":"record_pending", "params": [], "id": 1}"#).unwrap();

		let incomplete = incomplete.clone();
    	move |_| {
			assert_eq!(incomplete.load(Ordering::SeqCst), 0);
	    	out.send(r#"{"jsonrpc":"2.0", "method":"record_pending", "params": [], "id": 2}"#).unwrap();
			out.close(CloseCode::Normal)
		}
	}).unwrap();

	// then
	let mut i = 0;
	while incomplete.load(Ordering::SeqCst) != 1 && i < 10 {
		thread::sleep(Duration::from_millis(50));
		i += 1;
	}
	assert_eq!(incomplete.load(Ordering::SeqCst), 1);

}

#[test]
fn bind_port_zero_should_give_random_port() {
	let (server, _) = serve(0);

	assert_eq!(Ipv4Addr::new(127, 0, 0, 1), server.addr().ip());
	assert_ne!(0, server.addr().port());
}

#[test]
fn close_handle_makes_wait_return() {
	let (server, _) = serve(0);
	let close_handle = server.close_handle();

	let (tx, rx) = mpsc::channel();
	thread::spawn(move || {
		tx.send(server.wait()).unwrap();
	});
	thread::sleep(Duration::from_secs(1));
	close_handle.close();

	let result = rx.recv_timeout(Duration::from_secs(10)).expect("Expected server to close");
	assert!(result.is_ok());
}
