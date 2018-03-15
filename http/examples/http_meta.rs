extern crate jsonrpc_http_server;

use jsonrpc_http_server::{ServerBuilder, hyper, RestApi};
use jsonrpc_http_server::jsonrpc_core::*;
use self::hyper::header;

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
			Ok(Value::String("Please send a valid Bearer token in Authorization header.".to_owned()))
		}
	});

	let server = ServerBuilder::new(io)
		.rest_api(RestApi::Unsecure)
		// You can also implement `MetaExtractor` trait and pass a struct here.
		.meta_extractor(|req: &hyper::Request| {
			let auth = req.headers()
				.get::<header::Authorization<header::Bearer>>();
			let auth = auth.map(|h| h.token.clone());

			Meta { auth }
		})
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait();
}

