# jsonrpc-http-server
Rust http server using JSON-RPC 2.0.

[Documentation](http://ethcore.github.io/jsonrpc/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-http-server = { git = "https://github.com/ethcore/jsonrpc-http-server" }
```

`main.rs`

```rust
extern crate jsonrpc_core;
extern crate jsonrpc_http_server;

use std::sync::Arc;
use jsonrpc_core::*;
use jsonrpc_http_server::*;

struct SayHello;
impl MethodCommand for SayHello {
    fn execute(&self, _params: Params) -> Result<Value, Error> {
        Ok(Value::String("hello".to_string()))
    }
}

fn main() {
    let io = IoHandler::new();
    io.add_method("say_hello", SayHello);

    let server = ServerBuilder::new(Arc::new(io))
			.cors(DomainsValidation::AllowOnly(vec![AccessControlAllowOrigin::Null]))
			.start_http(&"127.0.0.1:3030".parse().unwrap())
			.expect("Unable to start RPC server");

		server.wait().unwrap();
}
```
