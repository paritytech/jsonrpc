# jsonrpc-core
Rust http server using JSON-RPC 2.0.

## Example

`Cargo.toml`


```
[dependencies]
jsonrpc-http-server = "0.1"
```

`main.rs`

```rust
extern crate jsonrpc_core;
extern crate jsonrpc_http_server;
 
use jsonrpc_core::*;
use jsonrpc_http_server::*;
 
struct SayHello;
impl MethodCommand for SayHello {
    fn execute(&mut self, _params: Option<Params>) -> Result<Value, Error> {
        Ok(Value::String("hello".to_string()))
    }
}
 
fn main() {
    let mut io = IoHandler::new();
    io.add_method("say_hello", SayHello);
    let server = Server::new(io, 1);
    server.start("127.0.0.1:3030".to_string());
}
```
