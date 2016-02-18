# jsonrpc-core
Transport agnostic rust implementation of JSON-RPC 2.0 Specification.

[![Build Status][travis-image]][travis-url]

[travis-image]: https://travis-ci.org/debris/jsonrpc-core.svg?branch=master
[travis-url]: https://travis-ci.org/debris/jsonrpc-core

[Documentation](http://debris.github.io/jsonrpc-core/jsonrpc_core/index.html)

- [x] - server side
- [ ] - client side *(contributions are welcomed!)*

## Example

`Cargo.toml`


```
[dependencies]
jsonrpc-core = "1.1"
```

`main.rs`

```rust
extern crate jsonrpc_core;

use jsonrpc_core::*;

struct SayHello;
impl MethodCommand for SayHello {
    fn execute(&mut self, _params: Params) -> Result<Value, Error> {
        Ok(Value::String("hello".to_string()))
    }
}

fn main() {
	let mut io = IoHandler::new();
	io.add_method("say_hello", SayHello);

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	assert_eq!(io.handle_request(request), Some(response.to_string()));
}
```
