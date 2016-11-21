extern crate jsonrpc_core;
extern crate jsonrpc_http_server;

use std::sync::Arc;
use jsonrpc_core::*;
use jsonrpc_http_server::*;

struct SayHello;
impl SyncMethodCommand for SayHello {
	fn execute(&self, _params: Params) -> Result<Value, Error> {
		Ok(Value::String("hello".to_string()))
	}
}

fn main() {
	let io = IoHandler::new();
	io.add_method("say_hello", SayHello);

	let server = ServerBuilder::new(Arc::new(io))
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

