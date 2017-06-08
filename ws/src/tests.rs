use std::io::{Read, Write};
use std::net::{TcpStream, Ipv4Addr};
use std::str::Lines;
use std::sync::mpsc;
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

fn serve(port: u16) -> Server {
	let mut io = core::IoHandler::default();
	io.add_method("hello", |_params: core::Params| Ok(core::Value::String("world".into())));
	io.add_async_method("hello_async", |_params: core::Params| {
		core::futures::finished(core::Value::String("world".into())).boxed()
	});

	ServerBuilder::new(io)
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
		.unwrap()
}

#[test]
fn should_disallow_not_whitelisted_origins() {
	// given
	let server = serve(30001);

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
	let server = serve(30002);

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
	let server = serve(30003);

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
	let server = serve(30004);

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
fn bind_port_zero_should_give_random_port() {
	let server = serve(0);

	assert_eq!(Ipv4Addr::new(127, 0, 0, 1), server.addr().ip());
	assert_ne!(0, server.addr().port());
}

#[test]
fn close_handle_makes_wait_return() {
	let server = serve(0);
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
