extern crate jsonrpc_core;

use std::str::Lines;
use std::net::TcpStream;
use std::io::{Read, Write};
use self::jsonrpc_core::{IoHandler, Params, Value, Error};

use self::jsonrpc_core::futures::{self, Future};
use super::*;

fn serve_hosts(hosts: Vec<Host>) -> Server {
	ServerBuilder::new(IoHandler::default())
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Value("parity.io".into())]))
		.allowed_hosts(DomainsValidation::AllowOnly(hosts))
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

fn serve() -> Server {
	serve_rest(RestApi::Secure, false)
}

fn serve_rest(rest: RestApi, cors_all: bool) -> Server {
	use std::thread;
	let mut io = IoHandler::default();
	io.add_method("hello", |params: Params| {
		match params.parse::<(u64, )>() {
			Ok((num, )) => Ok(Value::String(format!("world: {}", num))),
			_ => Ok(Value::String("world".into())),
		}
	});
	io.add_method("hello_async", |_params: Params| {
		futures::finished(Value::String("world".into()))
	});
	io.add_method("hello_async2", |_params: Params| {
		let (c, p) = futures::oneshot();
		thread::spawn(move || {
			thread::sleep(::std::time::Duration::from_millis(10));
			c.send(Value::String("world".into())).unwrap();
		});
		p.map_err(|_| Error::invalid_request())
	});

	ServerBuilder::new(io)
		.cors(if cors_all {
			DomainsValidation::Disabled
		} else {
			DomainsValidation::AllowOnly(vec![
				AccessControlAllowOrigin::Value("parity.io".into()),
				AccessControlAllowOrigin::Null,
			])
		})
		.rest_api(rest)
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

struct Response {
	status: String,
	headers: String,
	body: String,
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

fn request(server: Server, request: &str) -> Response {
	let mut req = TcpStream::connect(server.address()).unwrap();
	req.write_all(request.as_bytes()).unwrap();

	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();

	let mut lines = response.lines();
	let status = lines.next().unwrap().to_owned();
	let headers =	read_block(&mut lines);
	let body = read_block(&mut lines);

	Response {
		status: status,
		headers: headers,
		body: body,
	}
}

#[test]
fn should_return_method_not_allowed_for_get() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			I shouldn't be read.\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 405 Method Not Allowed".to_owned());
	assert_eq!(response.body, "3D\nUsed HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned());
}

#[test]
fn should_return_unsupported_media_type_if_not_json() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 415 Unsupported Media Type".to_owned());
	assert_eq!(response.body, "51\nSupplied content type is not allowed. Content-Type: application/json is required\n".to_owned());
}

#[test]
fn should_return_error_for_malformed_request() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"3.0","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_error_for_malformed_request2() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","metho1d":""}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_empty_response_for_notification() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, "0\n".to_owned());
}


#[test]
fn should_return_method_not_found() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_add_cors_headers() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: http://parity.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(response.headers.contains("Access-Control-Allow-Origin: http://parity.io"), "Headers missing in {}", response.headers);
}

#[test]
fn should_not_add_cors_headers() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: fake.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid());
}

#[test]
fn should_not_process_the_request_in_case_of_invalid_cors() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"hello"}"#;
	let response = request(server,
		&format!("\
			OPTIONS / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: fake.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid());
}


#[test]
fn should_return_proper_headers_on_options() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			OPTIONS / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Length: 0\r\n\
			\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert!(response.headers.contains("Allow: OPTIONS, POST"), "Headers missing in {}", response.headers);
	assert!(response.headers.contains("Accept: application/json"), "Headers missing in {}", response.headers);
	assert_eq!(response.body, "0\n");
}

#[test]
fn should_add_cors_header_for_null_origin() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: null\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(response.headers.contains("Access-Control-Allow-Origin: null"), "Headers missing in {}", response.headers);
}

#[test]
fn should_add_cors_header_for_null_origin_when_all() {
	// given
	let server = serve_rest(RestApi::Secure, true);

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: null\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(response.headers.contains("Access-Control-Allow-Origin: null"), "Headers missing in {}", response.headers);
}

#[test]
fn should_not_allow_request_larger_than_max() {
	let server = ServerBuilder::new(IoHandler::default())
		.max_request_body_size(7)
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap();

	let response = request(server,
		"\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Length: 8\r\n\
			Content-Type: application/json\r\n\
			\r\n\
			12345678\r\n\
		"
	);
	assert_eq!(response.status, "HTTP/1.1 413 Payload Too Large".to_owned());
}

#[test]
fn should_reject_invalid_hosts() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, invalid_host());
}

#[test]
fn should_reject_missing_host() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, invalid_host());
}

#[test]
fn should_allow_if_host_is_valid() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: parity.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_allow_application_json_utf8() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: parity.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json; charset=utf-8\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_always_allow_the_bind_address() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: {}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr, req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_always_allow_the_bind_address_as_localhost() {
	// given
	let server = serve_hosts(vec![]);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_handle_sync_requests_correctly() {
	// given
	let server = serve();
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"hello"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_with_immediate_response_correctly() {
	// given
	let server = serve();
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"hello_async"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_correctly() {
	// given
	let server = serve();
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"hello_async2"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_sync_batch_requests_correctly() {
	// given
	let server = serve();
	let addr = server.address().clone();

	// when
	let req = r#"[{"jsonrpc":"2.0","id":"1","method":"hello"}]"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_batch());
}

#[test]
fn should_handle_rest_request_with_params() {
	// given
	let server = serve();
	let addr = server.address().clone();

	// when
	let req = "";
	let response = request(server,
		&format!("\
			POST /hello/5 HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_5());
}

#[test]
fn should_return_error_in_case_of_unsecure_rest_and_no_method() {
	// given
	let server = serve_rest(RestApi::Unsecure, false);
	let addr = server.address().clone();

	// when
	let req = "";
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: localhost:{}\r\n\
			Connection: close\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", addr.port(), req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 415 Unsupported Media Type".to_owned());
	assert_eq!(&response.body, "51\nSupplied content type is not allowed. Content-Type: application/json is required\n");
}

fn invalid_host() -> String {
	"29\nProvided Host header is not whitelisted.\n".into()
}

fn cors_invalid() -> String {
	"76\nOrigin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n".into()
}

fn method_not_found() -> String {
 "4E\n{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32601,\"message\":\"Method not found\"},\"id\":1}\n".into()
}

fn invalid_request() -> String {
 "50\n{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32600,\"message\":\"Invalid request\"},\"id\":null}\n".into()
}
fn world() -> String {
 "2A\n{\"jsonrpc\":\"2.0\",\"result\":\"world\",\"id\":1}\n".into()
}
fn world_5() -> String {
 "2D\n{\"jsonrpc\":\"2.0\",\"result\":\"world: 5\",\"id\":1}\n".into()
}
fn world_batch() -> String {
 "2C\n[{\"jsonrpc\":\"2.0\",\"result\":\"world\",\"id\":1}]\n".into()
}
