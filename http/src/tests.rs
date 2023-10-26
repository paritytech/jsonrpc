use jsonrpc_core;

use self::jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Result, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::Lines;
use std::time::Duration;

use self::jsonrpc_core::futures;
use super::*;

fn serve_hosts(hosts: Vec<Host>) -> Server {
	ServerBuilder::new(IoHandler::default())
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Value(
			"parity.io".into(),
		)]))
		.allowed_hosts(DomainsValidation::AllowOnly(hosts))
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

fn id<T>(t: T) -> T {
	t
}

fn serve<F: FnOnce(ServerBuilder) -> ServerBuilder>(alter: F) -> Server {
	let builder = ServerBuilder::new(io())
		.cors(DomainsValidation::AllowOnly(vec![
			AccessControlAllowOrigin::Value("parity.io".into()),
			AccessControlAllowOrigin::Null,
		]))
		.cors_max_age(None)
		.rest_api(RestApi::Secure)
		.health_api(("/health", "hello_async"));

	alter(builder).start_http(&"127.0.0.1:0".parse().unwrap()).unwrap()
}

fn serve_allow_headers(cors_allow_headers: cors::AccessControlAllowHeaders) -> Server {
	let mut io = IoHandler::default();
	io.add_sync_method("hello", |params: Params| match params.parse::<(u64,)>() {
		Ok((num,)) => Ok(Value::String(format!("world: {}", num))),
		_ => Ok(Value::String("world".into())),
	});
	ServerBuilder::new(io)
		.cors(DomainsValidation::AllowOnly(vec![
			AccessControlAllowOrigin::Value("parity.io".into()),
			AccessControlAllowOrigin::Null,
		]))
		.cors_allow_headers(cors_allow_headers)
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap()
}

fn io() -> IoHandler {
	let mut io = IoHandler::default();
	io.add_sync_method("hello", |params: Params| match params.parse::<(u64,)>() {
		Ok((num,)) => Ok(Value::String(format!("world: {}", num))),
		_ => Ok(Value::String("world".into())),
	});
	io.add_sync_method("fail", |_: Params| -> Result<i64> {
		Err(Error::new(ErrorCode::ServerError(-34)))
	});
	io.add_method("hello_async", |_params: Params| {
		futures::future::ready(Ok(Value::String("world".into())))
	});
	io.add_method("hello_async2", |_params: Params| {
		use futures::TryFutureExt;
		let (c, p) = futures::channel::oneshot::channel();
		thread::spawn(move || {
			thread::sleep(Duration::from_millis(10));
			c.send(Value::String("world".into())).unwrap();
		});
		p.map_err(|_| Error::invalid_request())
	});

	io
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
			}
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
	let headers = read_block(&mut lines);
	let body = read_block(&mut lines);

	Response { status, headers, body }
}

#[test]
fn should_return_method_not_allowed_for_get() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		"\
		 GET / HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 \r\n\
		 I shouldn't be read.\r\n\
		 ",
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 405 Method Not Allowed".to_owned());
	assert_eq!(
		response.body,
		"Used HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned()
	);
}

#[test]
fn should_handle_health_endpoint() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		"\
		 GET /health HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 \r\n\
		 I shouldn't be read.\r\n\
		 ",
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, "\"world\"\n");
}

#[test]
fn should_handle_health_endpoint_failure() {
	// given
	let server = serve(|builder| builder.health_api(("/api/health", "fail")));

	// when
	let response = request(
		server,
		"\
		 GET /api/health HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 \r\n\
		 I shouldn't be read.\r\n\
		 ",
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 503 Service Unavailable".to_owned());
	assert_eq!(response.body, "{\"code\":-34,\"message\":\"Server error\"}\n");
}

#[test]
fn should_return_unsupported_media_type_if_not_json() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		"\
		 POST / HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 \r\n\
		 {}\r\n\
		 ",
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 415 Unsupported Media Type".to_owned());
	assert_eq!(
		response.body,
		"Supplied content type is not allowed. Content-Type: application/json is required\n".to_owned()
	);
}

#[test]
fn should_return_error_for_malformed_request() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"3.0","method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_error_for_malformed_request2() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","metho1d":""}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_empty_response_for_notification() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, "".to_owned());
}

#[test]
fn should_return_method_not_found() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_add_cors_allow_origins() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(
		response
			.headers
			.contains("access-control-allow-origin: http://parity.io"),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_add_cors_max_age_headers() {
	// given
	let server = serve(|builder| builder.cors_max_age(1_000));

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(
		response
			.headers
			.contains("access-control-allow-origin: http://parity.io"),
		"Headers missing in {}",
		response.headers
	);
	assert!(
		response.headers.contains("access-control-max-age: 1000"),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_not_add_cors_allow_origins() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: fake.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid_allow_origin());
}

#[test]
fn should_not_process_the_request_in_case_of_invalid_allow_origin() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#;
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: fake.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid_allow_origin());
}

#[test]
fn should_return_proper_headers_on_options() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		"\
		 OPTIONS / HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 Content-Length: 0\r\n\
		 \r\n\
		 ",
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert!(
		response.headers.contains("allow: OPTIONS, POST"),
		"Headers missing in {}",
		response.headers
	);
	assert!(
		response.headers.contains("accept: application/json"),
		"Headers missing in {}",
		response.headers
	);
	assert_eq!(response.body, "");
}

#[test]
fn should_add_cors_allow_origin_for_null_origin() {
	// given
	let server = serve(id);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: null\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(
		response.headers.contains("access-control-allow-origin: null"),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_add_cors_allow_origin_for_null_origin_when_all() {
	// given
	let server = serve(|builder| builder.cors(DomainsValidation::Disabled));

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: null\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(
		response.headers.contains("access-control-allow-origin: null"),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_not_allow_request_larger_than_max() {
	let server = ServerBuilder::new(IoHandler::default())
		.max_request_body_size(7)
		.start_http(&"127.0.0.1:0".parse().unwrap())
		.unwrap();

	let response = request(
		server,
		"\
		 POST / HTTP/1.1\r\n\
		 Host: 127.0.0.1:8080\r\n\
		 Connection: close\r\n\
		 Content-Length: 8\r\n\
		 Content-Type: application/json\r\n\
		 \r\n\
		 12345678\r\n\
		 ",
	);
	assert_eq!(response.status, "HTTP/1.1 413 Payload Too Large".to_owned());
}

#[test]
fn should_reject_invalid_hosts() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
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
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
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
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: parity.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_respond_configured_allowed_hosts_to_options() {
	// given
	let allowed = vec!["X-Allowed".to_owned(), "X-AlsoAllowed".to_owned()];
	let custom = cors::AccessControlAllowHeaders::Only(allowed.clone());
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Access-Control-Request-Headers: {}\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 \r\n\
			 ",
			&allowed.join(", ")
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let expected = format!("access-control-allow-headers: {}", &allowed.join(", "));
	assert!(
		response.headers.contains(&expected),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_not_contain_default_cors_allow_headers() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: 0\r\n\
			 \r\n\
			 "
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert!(
		!response.headers.contains("access-control-allow-headers:"),
		"Header should not be in {}",
		response.headers
	);
}

#[test]
fn should_respond_valid_to_default_allowed_headers() {
	// given
	let server = serve(id);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: Accept, Content-Type, Origin\r\n\
			 \r\n\
			 "
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let expected = "access-control-allow-headers: Accept, Content-Type, Origin";
	assert!(
		response.headers.contains(expected),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_by_default_respond_valid_to_any_request_headers() {
	// given
	let allowed = vec!["X-Abc".to_owned(), "X-123".to_owned()];
	let custom = cors::AccessControlAllowHeaders::Only(allowed.clone());
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: {}\r\n\
			 \r\n\
			 ",
			&allowed.join(", ")
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let expected = format!("access-control-allow-headers: {}", &allowed.join(", "));
	assert!(
		response.headers.contains(&expected),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_respond_valid_to_configured_allow_headers() {
	// given
	let allowed = vec!["X-Allowed".to_owned(), "X-AlsoAllowed".to_owned()];
	let custom = cors::AccessControlAllowHeaders::Only(allowed.clone());
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: {}\r\n\
			 \r\n\
			 ",
			&allowed.join(", ")
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let expected = format!("access-control-allow-headers: {}", &allowed.join(", "));
	assert!(
		response.headers.contains(&expected),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_respond_invalid_if_non_allowed_header_used() {
	// given
	let custom = cors::AccessControlAllowHeaders::Only(vec!["X-Allowed".to_owned()]);
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 X-Not-Allowed: not allowed\r\n\
			 \r\n\
			 "
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid_allow_headers());
}

#[test]
fn should_respond_valid_if_allowed_header_used() {
	// given
	let custom = cors::AccessControlAllowHeaders::Only(vec!["X-Allowed".to_owned()]);
	let server = serve_allow_headers(custom);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 X-Allowed: Foobar\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_respond_valid_if_case_insensitive_allowed_header_used() {
	// given
	let custom = cors::AccessControlAllowHeaders::Only(vec!["X-Allowed".to_owned()]);
	let server = serve_allow_headers(custom);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 X-AlLoWed: Foobar\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_respond_valid_on_case_mismatches_in_allowed_headers() {
	// given
	let allowed = vec!["X-Allowed".to_owned(), "X-AlsoAllowed".to_owned()];
	let custom = cors::AccessControlAllowHeaders::Only(allowed.clone());
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: x-ALLoweD, x-alSOaLloWeD\r\n\
			 \r\n\
			 "
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let contained = response
		.headers
		.contains("access-control-allow-headers: x-ALLoweD, x-alSOaLloWeD");
	assert!(contained, "Headers missing in {}", response.headers);
}

#[test]
fn should_respond_valid_to_any_requested_header() {
	// given
	let custom = cors::AccessControlAllowHeaders::Any;
	let server = serve_allow_headers(custom);
	let headers = "Something, Anything, Xyz, 123, _?";

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: {}\r\n\
			 \r\n\
			 ",
			headers
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	let expected = format!("access-control-allow-headers: {}", headers);
	assert!(
		response.headers.contains(&expected),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_forbid_invalid_request_headers() {
	// given
	let custom = cors::AccessControlAllowHeaders::Only(vec!["X-Allowed".to_owned()]);
	let server = serve_allow_headers(custom);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: *\r\n\
			 \r\n\
			 "
		),
	);

	// then
	// According to the spec wildcard is nly supported for `Allow-Origin`,
	// some ppl believe it should be supported by other `Allow-*` headers,
	// but I didn't see any mention of allowing wildcard for `Request-Headers`.
	assert_eq!(response.status, "HTTP/1.1 403 Forbidden".to_owned());
	assert_eq!(response.body, cors_invalid_allow_headers());
}

#[test]
fn should_respond_valid_to_wildcard_if_any_header_allowed() {
	// given
	let server = serve_allow_headers(cors::AccessControlAllowHeaders::Any);

	// when
	let response = request(
		server,
		&format!(
			"\
			 OPTIONS / HTTP/1.1\r\n\
			 Host: 127.0.0.1:8080\r\n\
			 Origin: http://parity.io\r\n\
			 Content-Length: 0\r\n\
			 Content-Type: application/json\r\n\
			 Connection: close\r\n\
			 Access-Control-Request-Headers: *\r\n\
			 \r\n\
			 "
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert!(
		response.headers.contains("access-control-allow-headers: *"),
		"Headers missing in {}",
		response.headers
	);
}

#[test]
fn should_allow_application_json_utf8() {
	// given
	let server = serve_hosts(vec!["parity.io".into()]);

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: parity.io\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json; charset=utf-8\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			req.as_bytes().len(),
			req
		),
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
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: {}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr,
			req.as_bytes().len(),
			req
		),
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
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_handle_sync_requests_correctly() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_with_immediate_response_correctly() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello_async"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_async_requests_correctly() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"hello_async2"}"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world());
}

#[test]
fn should_handle_sync_batch_requests_correctly() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = r#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_batch());
}

#[test]
fn should_handle_rest_request_with_params() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = "";
	let response = request(
		server,
		&format!(
			"\
			 POST /hello/5 HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_5());
}

#[test]
fn should_handle_rest_request_with_case_insensitive_content_type() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = "";
	let response = request(
		server,
		&format!(
			"\
			 POST /hello/5 HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: Application/JSON; charset=UTF-8\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_5());
}

#[test]
fn should_return_error_in_case_of_unsecure_rest_and_no_method() {
	// given
	let server = serve(|builder| builder.rest_api(RestApi::Unsecure));
	let addr = server.address().clone();

	// when
	let req = "";
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 415 Unsupported Media Type".to_owned());
	assert_eq!(
		&response.body,
		"Supplied content type is not allowed. Content-Type: application/json is required\n"
	);
}

#[test]
fn should_return_connection_header() {
	// given
	let server = serve(id);
	let addr = server.address().clone();

	// when
	let req = r#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: close\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert!(
		response.headers.contains("connection: close"),
		"Headers missing in {}",
		response.headers
	);
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_batch());
}

#[test]
fn close_handle_makes_wait_return() {
	let server = serve(id);
	let close_handle = server.close_handle();

	let (tx, rx) = mpsc::channel();

	thread::spawn(move || {
		tx.send(server.wait()).unwrap();
	});

	thread::sleep(Duration::from_secs(3));

	close_handle.close();

	rx.recv_timeout(Duration::from_secs(10))
		.expect("Expected server to close");
}

#[test]
fn should_close_connection_without_keep_alive() {
	// given
	let server = serve(|builder| builder.keep_alive(false));
	let addr = server.address().clone();

	// when
	let req = r#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert!(
		response.headers.contains("connection: close"),
		"Header missing in {}",
		response.headers
	);
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_batch());
}

#[test]
fn should_respond_with_close_even_if_client_wants_to_keep_alive() {
	// given
	let server = serve(|builder| builder.keep_alive(false));
	let addr = server.address().clone();

	// when
	let req = r#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#;
	let response = request(
		server,
		&format!(
			"\
			 POST / HTTP/1.1\r\n\
			 Host: localhost:{}\r\n\
			 Connection: keep-alive\r\n\
			 Content-Type: application/json\r\n\
			 Content-Length: {}\r\n\
			 \r\n\
			 {}\r\n\
			 ",
			addr.port(),
			req.as_bytes().len(),
			req
		),
	);

	// then
	assert!(
		response.headers.contains("connection: close"),
		"Headers missing in {}",
		response.headers
	);
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, world_batch());
}

#[test]
fn should_drop_io_handler_when_server_is_closed() {
	use std::sync::{Arc, Mutex};
	// given
	let (weak, _req) = {
		let my_ref = Arc::new(Mutex::new(5));
		let weak = Arc::downgrade(&my_ref);
		let mut io = IoHandler::default();
		io.add_sync_method("hello", move |_| {
			Ok(Value::String(format!("{}", my_ref.lock().unwrap())))
		});
		let server = ServerBuilder::new(io)
			.start_http(&"127.0.0.1:0".parse().unwrap())
			.unwrap();

		let addr = server.address().clone();

		// when
		let req = TcpStream::connect(addr).unwrap();
		server.close();
		(weak, req)
	};

	// then
	for _ in 1..1000 {
		if weak.upgrade().is_none() {
			return;
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
	}

	panic!("expected server to be closed and io handler to be dropped")
}

#[test]
fn should_not_close_server_when_serving_errors() {
	// given
	let server = serve(|builder| builder.keep_alive(false));
	let addr = server.address().clone();

	// when
	let req = "{}";
	let request = format!(
		"\
		 POST / HTTP/1.1\r\n\
		 Host: localhost:{}\r\n\
		 Content-Type: application/json\r\n\
		 Content-Length: {}\r\n\
		 😈: 😈\r\n\
		 \r\n\
		 {}\r\n\
		 ",
		addr.port(),
		req.as_bytes().len(),
		req
	);

	let mut req = TcpStream::connect(addr).unwrap();
	req.write_all(request.as_bytes()).unwrap();
	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();
	assert!(!response.is_empty(), "Response should not be empty: {}", response);

	// then make a second request and it must not fail.
	let mut req = TcpStream::connect(addr).unwrap();
	req.write_all(request.as_bytes()).unwrap();
	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();
	assert!(!response.is_empty(), "Response should not be empty: {}", response);
}

fn invalid_host() -> String {
	"Provided Host header is not whitelisted.\n".into()
}

fn cors_invalid_allow_origin() -> String {
	"Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n".into()
}

fn cors_invalid_allow_headers() -> String {
	"Requested headers are not allowed for CORS. CORS headers would not be sent and any side-effects were cancelled as well.\n".into()
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
fn world_5() -> String {
	"{\"jsonrpc\":\"2.0\",\"result\":\"world: 5\",\"id\":1}\n".into()
}
fn world_batch() -> String {
	"[{\"jsonrpc\":\"2.0\",\"result\":\"world\",\"id\":1}]\n".into()
}
