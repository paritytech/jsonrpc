extern crate jsonrpc_stdio_server;

use jsonrpc_stdio_server::ServerBuilder;
use jsonrpc_stdio_server::jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_owned()))
	});

	ServerBuilder::new(io).build();
}
