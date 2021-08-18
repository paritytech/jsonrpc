# jsonrpc-http-server
Rust http server using JSON-RPC 2.0.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_http_server/index.html)

## Example

`Cargo.toml`

```
[dependencies]
jsonrpc-http-server = "18.0"
```

`main.rs`

```rust
use jsonrpc_http_server::jsonrpc_core::{IoHandler, Value, Params};
use jsonrpc_http_server::ServerBuilder;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params: Params| async {
		Ok(Value::String("hello".to_owned()))
	});

	let server = ServerBuilder::new(io)
		.threads(3)
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.unwrap();

	server.wait();
}
```
You can now test the above server by running `cargo run` in one terminal, and from another terminal issue the following POST request to your server:
```
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "say_hello", "id":123 }' 127.0.0.1:3030
```
to which the server will respond with the following:
```
{"jsonrpc":"2.0","result":"hello","id":123}
```
If you omit any of the fields above, or invoke a different method you will get an informative error message:
```
$ curl -X POST -H "Content-Type: application/json" -d '{"method": "say_hello", "id":123 }' 127.0.0.1:3030
{"error":{"code":-32600,"message":"Unsupported JSON-RPC protocol version"},"id":123}
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "say_bye", "id":123 }' 127.0.0.1:3030
{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":123}
```
