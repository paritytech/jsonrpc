extern crate jsonrpc_core;
extern crate jsonrpc_http_server;


use jsonrpc_core::{IoHandler, Value};
use jsonrpc_core::futures::{self, Future};
use jsonrpc_http_server::{hyper, ServerBuilder, DomainsValidation, AccessControlAllowOrigin, Response};

fn main() {
	let mut io = IoHandler::default();
	io.add_async_method("say_hello", |_params| {
		futures::finished(Value::String("hello".to_owned())).boxed()
	});

	let server = ServerBuilder::new(io)
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.request_middleware(|request: &hyper::server::Request| {
			if request.path() == "/status" {
				Some(Response::ok("Server running OK."))
			} else {
				None
			}.into()
		})
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait();
}

