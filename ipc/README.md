# jsonrpc-ipc-server
IPC server (Windows & Linux) for JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_ipc_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-ipc-server = { git = "https://github.com/paritytech/jsonrpc" }
```

`main.rs`

```rust
extern crate jsonrpc_ipc_server;

use jsonrpc_ipc_server::Server;
use jsonrpc_ipc_server::jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::new();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".into()))
	});

	let server = Server::new("/tmp/json-ipc-test.ipc", io).unwrap();
	::std::thread::spawn(move || server.run());
}
```

