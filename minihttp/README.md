# jsonrpc-minihttp-server
Blazing fast HTTP server for JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-minihttp-server = { git = "https://github.com/paritytech/jsonrpc" }
```

`main.rs`

```rust
extern crate jsonrpc_minihttp_server;

use jsonrpc_minihttp_server::*;
use jsonrpc_minihttp_server::jsonrpc_core::*;
use jsonrpc_minihttp_server::cors::AccessControlAllowOrigin;

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
