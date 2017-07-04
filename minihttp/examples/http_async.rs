extern crate jsonrpc_core;
extern crate jsonrpc_minihttp_server;


use jsonrpc_core::*;
use jsonrpc_minihttp_server::{cors, ServerBuilder, DomainsValidation};

fn main() {
	let mut io = IoHandler::default();
	io.add_async_method("say_hello", |_params| {
		futures::finished(Value::String("hello".to_owned()))
	});

	let server = ServerBuilder::new(io)
		.cors(DomainsValidation::AllowOnly(vec![cors::AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

