extern crate jsonrpc_core;
extern crate env_logger;
extern crate reqwest;

use std::io::Read;
use self::reqwest::{StatusCode, Method};
use self::reqwest::header::{self, Headers};
use self::jsonrpc_core::{IoHandler, Params, Value, Error};
use self::jsonrpc_core::futures::{self, Future};
use super::{ServerBuilder, Server, cors, hosts};

fn serve_hosts(hosts: Vec<hosts::Host>) -> Server {
	let _ = env_logger::try_init();

	ServerBuilder::new(IoHandler::default())
		.cors(hosts::DomainsValidation::AllowOnly(vec![cors::AccessControlAllowOrigin::Value("http://parity.io".into())]))
		.allowed_hosts(hosts::DomainsValidation::AllowOnly(hosts))
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

fn serve() -> Server {
	use std::thread;

	let _ = env_logger::try_init();
	let mut io = IoHandler::default();
	io.add_method("hello", |_params: Params| Ok(Value::String("world".into())));
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
		.cors(hosts::DomainsValidation::AllowOnly(vec![
			cors::AccessControlAllowOrigin::Value("http://parity.io".into()),
			cors::AccessControlAllowOrigin::Null,
		]))
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

struct Response {
	pub status: reqwest::StatusCode,
	pub body: String,
	pub headers: Headers,
}

fn request(server: Server, method: Method, headers: Headers, body: &'static str) -> Response {
	let client = reqwest::Client::new().unwrap();
	let mut res = client.request(method, &format!("http://{}", server.address()))
		.headers(headers)
		.body(body)
		.send()
		.unwrap();

	let mut body = String::new();
	res.read_to_string(&mut body).unwrap();

	Response {
		status: res.status().clone(),
		body: body,
		headers: res.headers().clone(),
	}
}

#[test]
fn should_return_method_not_allowed_for_get() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Get,
		Headers::new(),
		"I shouldn't be read.",
	);

	// then
	assert_eq!(response.status, StatusCode::MethodNotAllowed);
	assert_eq!(response.body, "Used HTTP Method is not allowed. POST or OPTIONS is required.\n".to_owned());
}

#[test]
fn should_ignore_media_type_if_options() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Options,
		Headers::new(),
		"",
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(
		response.headers.get::<reqwest::header::Allow>(),
		Some(&reqwest::header::Allow(vec![Method::Options, Method::Post]))
	);
	assert!(response.headers.get::<reqwest::header::Accept>().is_some());
	assert_eq!(response.body, "");
}


#[test]
fn should_return_403_for_options_if_origin_is_invalid() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.append_raw("origin", b"http://invalid.io".to_vec());
			headers
		},
		""
	);

	// then
	assert_eq!(response.status, StatusCode::Forbidden);
	assert_eq!(response.body, cors_invalid());
}

#[test]
fn should_return_unsupported_media_type_if_not_json() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		Headers::new(),
		"{}",
	);

	// then
	assert_eq!(response.status, StatusCode::UnsupportedMediaType);
	assert_eq!(response.body, "Supplied content type is not allowed. Content-Type: application/json is required.\n".to_owned());
}

fn content_type_json() -> Headers {
	let mut headers = Headers::new();
	headers.set_raw("content-type", vec![b"application/json".to_vec()]);
	headers
}

#[test]
fn should_return_error_for_malformed_request() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"3.0","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_error_for_malformed_request2() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","metho1d":""}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_empty_response_for_notification() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, "\n".to_owned());
}


#[test]
fn should_return_method_not_found() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_add_cors_headers() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set(header::Origin::new("http", "parity.io", None));
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
	assert_eq!(
		response.headers.get::<reqwest::header::AccessControlAllowOrigin>(),
		Some(&reqwest::header::AccessControlAllowOrigin::Value("http://parity.io".into()))
	);
}

#[test]
fn should_add_cors_headers_for_options() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Options,
		{
			let mut headers = content_type_json();
			headers.set(header::Origin::new("http", "parity.io", None));
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, "".to_owned());
	println!("{:?}", response.headers);
	assert_eq!(
		response.headers.get::<reqwest::header::AccessControlAllowOrigin>(),
		Some(&reqwest::header::AccessControlAllowOrigin::Value("http://parity.io".into()))
	);
}

#[test]
fn should_not_process_request_with_invalid_cors() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set(header::Origin::new("http", "fake.io", None));
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Forbidden);
	assert_eq!(response.body, cors_invalid());
}

#[test]
fn should_add_cors_header_for_null_origin() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.append_raw("origin", b"null".to_vec());
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
	assert_eq!(
		response.headers.get::<reqwest::header::AccessControlAllowOrigin>().cloned(),
		Some(reqwest::header::AccessControlAllowOrigin::Null)
	);
}

#[test]
fn should_reject_invalid_hosts() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set_raw("Host", vec![b"127.0.0.1:8080".to_vec()]);
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Forbidden);
	assert_eq!(response.body, invalid_host());
}

#[test]
fn should_allow_if_host_is_valid() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set_raw("Host", vec![b"parity.io".to_vec()]);
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_always_allow_the_bind_address() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);
	let addr = server.address().clone();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set_raw("Host", vec![format!("{}", addr).as_bytes().to_vec()]);
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_always_allow_the_bind_address_as_localhost() {
	// given
	let server = serve_hosts(vec![]);
	let addr = server.address().clone();

	// when
	let response = request(server,
		Method::Post,
		{
			let mut headers = content_type_json();
			headers.set_raw("Host", vec![format!("localhost:{}", addr.port()).as_bytes().to_vec()]);
			headers
		},
		r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_handle_sync_requests_correctly() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","id":"1","method":"hello"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_with_immediate_response_correctly() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","id":"1","method":"hello_async"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_correctly() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"{"jsonrpc":"2.0","id":"1","method":"hello_async2"}"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_sync_batch_requests_correctly() {
	// given
	let server = serve();

	// when
	let response = request(server,
		Method::Post,
		content_type_json(),
		r#"[{"jsonrpc":"2.0","id":"1","method":"hello"}]"#,
	);

	// then
	assert_eq!(response.status, StatusCode::Ok);
	assert_eq!(response.body, world_batch());
}

fn invalid_host() -> String {
	"Provided Host header is not whitelisted.\n".into()
}

fn cors_invalid() -> String {
	"Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n".into()
}

fn method_not_found() -> String {
 "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32601,\"message\":\"Method not found\"},\"id\":1}\n".into()
}

fn invalid_request() -> String {
 "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32600,\"message\":\"Invalid request\"},\"id\":null}\n".into()
}
fn world() -> String {
 "{\"jsonrpc\":\"2.0\",\"result\":\"world\",\"id\":1}\n".into()
}
fn world_batch() -> String {
 "[{\"jsonrpc\":\"2.0\",\"result\":\"world\",\"id\":1}]\n".into()
}
