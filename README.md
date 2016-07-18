# jsonrpc-http-server
Rust http server using JSON-RPC 2.0.

[![Build Status][travis-image]][travis-url]

[travis-image]: https://travis-ci.org/ethcore/jsonrpc-http-server.svg?branch=master
[travis-url]: https://travis-ci.org/ethcore/jsonrpc-http-server

[Documentation](http://ethcore.github.io/jsonrpc-http-server/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`


```
[dependencies]
jsonrpc-http-server = "6.0"
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
    fn execute(&self, _params: Option<Params>) -> Result<Value, Error> {
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
}
```
