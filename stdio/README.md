# jsonrpc-stdio-server
STDIN/STDOUT server for JSON-RPC 2.0.
Takes one request per line and outputs each response on a new line.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_stdio_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-stdio-server = "14.0"
```

`main.rs`

```rust
use jsonrpc_stdio_server::server;
use jsonrpc_stdio_server::jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params| {
		Ok(Value::String("hello".to_owned()))
	});

	ServerBuilder::new(io).build();
}
```
