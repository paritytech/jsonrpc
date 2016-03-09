# jsonrpc-http-server
Rust http server using JSON-RPC 2.0.

[![Build Status][travis-image]][travis-url]

[travis-image]: https://travis-ci.org/debris/jsonrpc-http-server.svg?branch=master
[travis-url]: https://travis-ci.org/debris/jsonrpc-http-server

[Documentation](http://debris.github.io/jsonrpc-http-server/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`


```
[dependencies]
jsonrpc-http-server = "1.1"
```

`main.rs`

```rust
extern crate jsonrpc_core;
extern crate jsonrpc_http_server;
 
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
    let server = Server::new(Arc::new(io));
    server.start("127.0.0.1:3030".to_string(), AccessControlAllowOrigin::Null, 1);
}
```
