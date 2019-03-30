use jsonrpc_http_server::jsonrpc_core::*;
use jsonrpc_http_server::{cors::AccessControlAllowHeaders, hyper, RestApi, ServerBuilder};

#[derive(Default, Clone)]
struct Meta {
	auth: Option<String>,
}

impl Metadata for Meta {}

fn main() {
	let mut io = MetaIoHandler::default();

	io.add_method_with_meta("say_hello", |_params: Params, meta: Meta| {
		let auth = meta.auth.unwrap_or_else(String::new);
		if auth.as_str() == "let-me-in" {
			Ok(Value::String("Hello World!".to_owned()))
		} else {
			Ok(Value::String(
				"Please send a valid Bearer token in Authorization header.".to_owned(),
			))
		}
	});

	let server = ServerBuilder::new(io)
		.cors_allow_headers(AccessControlAllowHeaders::Only(vec!["Authorization".to_owned()]))
		.rest_api(RestApi::Unsecure)
		// You can also implement `MetaExtractor` trait and pass a struct here.
		.meta_extractor(|req: &hyper::Request<hyper::Body>| {
			let auth = req
				.headers()
				.get(hyper::header::AUTHORIZATION)
				.map(|h| h.to_str().unwrap_or("").to_owned());

			Meta { auth }
		})
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait();
}
