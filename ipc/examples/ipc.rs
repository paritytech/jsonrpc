extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server;

use jsonrpc_core::*;
use jsonrpc_ipc_server::IpcServer;

fn main() {
	let mut io = MetaIoHandler::<()>::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_string()))
	});

	let event_loop = jsonrpc_ipc_server::UninitializedRemote::Unspawned.initialize().unwrap();
	let remote = event_loop.remote();
	let _server = jsonrpc_ipc_server::Server::start(
		io,
		"/tmp/parity-example.ipc",
		remote,
		|_context: &jsonrpc_ipc_server::RequestContext| Default::default(),
	).expect("Server should start ok");
}

