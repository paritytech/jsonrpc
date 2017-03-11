extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server;

use jsonrpc_core::*;

fn main() {
	let mut io = MetaIoHandler::<()>::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_string()))
	});
	let _server = jsonrpc_ipc_server::server(io, "/tmp/parity-example.ipc").expect("Server should start ok");
}

