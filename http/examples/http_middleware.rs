extern crate jsonrpc_http_server;

use jsonrpc_http_server::{
	hyper, ServerBuilder, DomainsValidation, AccessControlAllowOrigin, Response, RestApi
};
use jsonrpc_http_server::jsonrpc_core::{IoHandler, Value};
use jsonrpc_http_server::jsonrpc_core::futures;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params| {
		futures::finished(Value::String("hello".to_owned()))
	});

	let server = ServerBuilder::new(io)
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.request_middleware(|request: hyper::server::Request| {
			if request.path() == "/status" {
				Response::ok("Server running OK.").into()
			} else {
				request.into()
			}
		})
		.rest_api(RestApi::Unsecure)
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait();
}

