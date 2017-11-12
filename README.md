# Parity JSON-RPC

Rust implementation of JSON-RPC 2.0 Specification.
Transport-agnostic `core` and transport servers for `http`, `ipc`, `websockets` and `tcp`.

[![Build Status][travis-image]][travis-url]

[travis-image]: https://travis-ci.org/paritytech/jsonrpc.svg?branch=master
[travis-url]: https://travis-ci.org/paritytech/jsonrpc

[Documentation](http://paritytech.github.io/jsonrpc/)

## Sub-projects
- [jsonrpc-core](./core) [![crates.io][core-image]][core-url]
- [jsonrpc-http-server](./http) [![crates.io][http-server-image]][http-server-url]
- [jsonrpc-minihttp-server](./minihttp)
- [jsonrpc-ipc-server](./ipc)
- [jsonrpc-tcp-server](./tcp) [![crates.io][tcp-server-image]][tcp-server-url]
- [jsonrpc-ws-server](./ws)
- [jsonrpc-macros](./macros) [![crates.io][macros-image]][macros-url]
- [jsonrpc-server-utils](./server-utils) [![crates.io][server-utils-image]][server-utils-url]
- [jsonrpc-pubsub](./pubsub) [![crates.io][pubsub-image]][pubsub-url]

[core-image]: https://img.shields.io/crates/v/jsonrpc-core.svg
[core-url]: https://crates.io/crates/jsonrpc-core
[http-server-image]: https://img.shields.io/crates/v/jsonrpc-http-server.svg
[http-server-url]: https://crates.io/crates/jsonrpc-http-server
[tcp-server-image]: https://img.shields.io/crates/v/jsonrpc-tcp-server.svg
[tcp-server-url]: https://crates.io/crates/jsonrpc-tcp-server
[macros-image]: https://img.shields.io/crates/v/jsonrpc-macros.svg
[macros-url]: https://crates.io/crates/jsonrpc-macros
[server-utils-image]: https://img.shields.io/crates/v/jsonrpc-server-utils.svg
[server-utils-url]: https://crates.io/crates/jsonrpc-server-utils
[pubsub-image]: https://img.shields.io/crates/v/jsonrpc-pubsub.svg
[pubsub-url]: https://crates.io/crates/jsonrpc-pubsub

## Examples

- [core](./core/examples)
- [macros](./macros/examples)
- [pubsub](./pubsub/examples)

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

use jsonrpc_core::Result;

build_rpc_trait! {
	pub trait Rpc {
		/// Adds two numbers and returns a result
		#[rpc(name = "add")]
		fn add(&self, u64, u64) -> Result<u64>;
	}
}

pub struct RpcImpl;
impl Rpc for RpcImpl {
	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}
}


fn main() {
	let mut io = jsonrpc_core::IoHandler::new();
	io.extend_with(RpcImpl.to_delegate())
}
