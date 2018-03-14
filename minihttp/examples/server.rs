extern crate jsonrpc_minihttp_server;

use jsonrpc_minihttp_server::{cors, ServerBuilder, DomainsValidation};
use jsonrpc_minihttp_server::jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params: Params| {
		Ok(Value::String("hello".to_string()))
	});

	let server = ServerBuilder::new(io)
		.threads(3)
		.cors(DomainsValidation::AllowOnly(vec![cors::AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

