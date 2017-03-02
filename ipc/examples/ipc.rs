extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server;

use jsonrpc_core::*;
use jsonrpc_ipc_server::Server;

fn main() {
	let mut io = IoHandler::new();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".into()))
	});

	let server = Server::new("/tmp/json-ipc-test.ipc", io).unwrap();
	::std::thread::spawn(move || server.run());
}

