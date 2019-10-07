# jsonrpc-tcp-server
TCP server for JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_tcp_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-tcp-server = "14.0"
```

`main.rs`

```rust
use jsonrpc_tcp_server::*;
use jsonrpc_tcp_server::jsonrpc_core::*;

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


