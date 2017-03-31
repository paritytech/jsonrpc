# Parity JSON-RPC

Rust implementation of JSON-RPC 2.0 Specification.
Transport-agnostic `core` and transport servers for `http`, `ipc`, `websockets` and `tcp`.

[![Build Status][travis-image]][travis-url]

[travis-image]: https://travis-ci.org/paritytech/jsonrpc.svg?branch=master
[travis-url]: https://travis-ci.org/paritytech/jsonrpc

[Documentation](http://paritytech.github.io/jsonrpc/jsonrpc/index.html)

## Sub-projects
- [jsonrpc-core](./core)
- [jsonrpc-http-server](./http)
- [jsonrpc-minihttp-server](./minihttp)
- [jsonrpc-ipc-server](./ipc)
- [jsonrpc-tcp-server](./tcp)
- [jsonrpc-ws-server](./ws)
- [jsonrpc-macros](./macros)

## Examples

- [Core](./core/examples)
- [Macros](./macros/examples)

### Basic Usage (with HTTP transport)

```rust
extern crate jsonrpc_core;
extern crate jsonrpc_minihttp_server;

use jsonrpc_core::{IoHandler, Value, Params};
use jsonrpc_minihttp_server::{ServerBuilder};

fn main() {
	let mut io = IoHandler::new();
	io.add_method("say_hello", |_params: Params| {
		Ok(Value::String("hello".to_string()))
	});

	let server = ServerBuilder::new(io)
		.threads(3)
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.unwrap();

	server.wait().unwrap();
}
```

### Basic usage with macros

```rust
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_macros;

use jsonrpc_core::Error;

build_rpc_trait! {
	pub trait Rpc {
		/// Adds two numbers and returns a result
		#[rpc(name = "add")]
		fn add(&self, u64, u64) -> Result<u64, Error>;
	}
}

pub struct RpcImpl;
impl Rpc for RpcImpl {
	fn add(&self, a: u64, b: u64) -> Result<u64, Error> {
		Ok(a + b)
	}
}


fn main() {
	let mut io = jsonrpc_core::IoHandler::new();
	io.extend_with(RpcImpl.to_delegate())
}
