# jsonrpc-tcp-server
TCP server for JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_tcp_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-core = "6.0"
jsonrpc-tcp-server = "6.0"
```

`main.rs`

```rust
extern crate jsonrpc_core;
extern crate jsonrpc_tcp_server;

use jsonrpc_core::*;
use jsonrpc_tcp_server::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_owned()))
	});

	let server = ServerBuilder::new(io)
		.start(&"0.0.0.0:3030".parse().unwrap())
		.expect("Server must start with no issues");

	server.wait().unwrap()
}
```


