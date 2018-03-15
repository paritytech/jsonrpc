# jsonrpc-http-server
Rust http server using JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-http-server = { git = "https://github.com/paritytech/jsonrpc" }
```

`main.rs`

```rust
extern crate jsonrpc_http_server;

use jsonrpc_http_server::*;
use jsonrpc_http_server::jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_| {
		Ok(Value::String("hello".into()))
	});

	let server = ServerBuilder::new(io)
		.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}
```
