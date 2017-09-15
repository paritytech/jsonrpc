# jsonrpc-core
Transport agnostic rust implementation of JSON-RPC 2.0 Specification.

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc_core/index.html)

- [x] - server side
- [x] - client side

## Example

`Cargo.toml`


```
[dependencies]
jsonrpc-core = "4.0"
```

`main.rs`

```rust
extern crate jsonrpc_core;

use jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::default();
	io.add_method("say_hello", |_params: Params| {
		Ok(Value::String("hello".into()))
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}
```

### Asynchronous responses

`main.rs`

```rust
extern crate jsonrpc_core;

use jsonrpc_core::*;
use jsonrpc_core::futures::Future;

fn main() {
	let io = IoHandler::new();
	io.add_async_method("say_hello", |_params: Params| {
		futures::finished(Value::String("hello".into()))
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	assert_eq!(io.handle_request(request).wait().unwrap(), Some(response.to_owned()));
}
```

### Publish-Subscribe
See examples directory.
