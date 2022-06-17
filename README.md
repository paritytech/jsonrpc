# Parity JSON-RPC

**NOTE: This crate is no longer actively developed; please have a look at our 
[jsonrpsee](https://github.com/paritytech/jsonrpsee) crate if you're looking for an actively 
maintained JSON RPC implementation.**

Rust implementation of JSON-RPC 2.0 Specification.
Transport-agnostic `core` and transport servers for `http`, `ipc`, `websockets` and `tcp`.

**New!** Support for [clients](#Client-support).

[Documentation](https://docs.rs/jsonrpc-core/)

## Sub-projects
- [jsonrpc-core](./core) [![crates.io][core-image]][core-url]
- [jsonrpc-core-client](./core-client) [![crates.io][core-client-image]][core-client-url]
- [jsonrpc-http-server](./http) [![crates.io][http-server-image]][http-server-url]
- [jsonrpc-ipc-server](./ipc) [![crates.io][ipc-server-image]][ipc-server-url]
- [jsonrpc-tcp-server](./tcp) [![crates.io][tcp-server-image]][tcp-server-url]
- [jsonrpc-ws-server](./ws) [![crates.io][ws-server-image]][ws-server-url]
- [jsonrpc-stdio-server](./stdio) [![crates.io][stdio-server-image]][stdio-server-url]
- [jsonrpc-derive](./derive) [![crates.io][derive-image]][derive-url]
- [jsonrpc-server-utils](./server-utils) [![crates.io][server-utils-image]][server-utils-url]
- [jsonrpc-pubsub](./pubsub) [![crates.io][pubsub-image]][pubsub-url]

[core-image]: https://img.shields.io/crates/v/jsonrpc-core.svg
[core-url]: https://crates.io/crates/jsonrpc-core
[core-client-image]: https://img.shields.io/crates/v/jsonrpc-core-client.svg
[core-client-url]: https://crates.io/crates/jsonrpc-core-client
[http-server-image]: https://img.shields.io/crates/v/jsonrpc-http-server.svg
[http-server-url]: https://crates.io/crates/jsonrpc-http-server
[ipc-server-image]: https://img.shields.io/crates/v/jsonrpc-ipc-server.svg
[ipc-server-url]: https://crates.io/crates/jsonrpc-ipc-server
[tcp-server-image]: https://img.shields.io/crates/v/jsonrpc-tcp-server.svg
[tcp-server-url]: https://crates.io/crates/jsonrpc-tcp-server
[ws-server-image]: https://img.shields.io/crates/v/jsonrpc-ws-server.svg
[ws-server-url]: https://crates.io/crates/jsonrpc-ws-server
[stdio-server-image]: https://img.shields.io/crates/v/jsonrpc-stdio-server.svg
[stdio-server-url]: https://crates.io/crates/jsonrpc-stdio-server
[derive-image]: https://img.shields.io/crates/v/jsonrpc-derive.svg
[derive-url]: https://crates.io/crates/jsonrpc-derive
[server-utils-image]: https://img.shields.io/crates/v/jsonrpc-server-utils.svg
[server-utils-url]: https://crates.io/crates/jsonrpc-server-utils
[pubsub-image]: https://img.shields.io/crates/v/jsonrpc-pubsub.svg
[pubsub-url]: https://crates.io/crates/jsonrpc-pubsub

## Examples

- [core](./core/examples)
- [derive](./derive/examples)
- [pubsub](./pubsub/examples)

### Basic Usage (with HTTP transport)

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

### Basic usage with derive

```rust
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;

#[rpc]
pub trait Rpc {
	/// Adds two numbers and returns a result
	#[rpc(name = "add")]
	fn add(&self, a: u64, b: u64) -> Result<u64>;
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
```

### Client support

```rust
use jsonrpc_core_client::transports::local;
use jsonrpc_core::{BoxFuture, IoHandler, Result};
use jsonrpc_core::futures::{self, future, TryFutureExt};
use jsonrpc_derive::rpc;

/// Rpc trait
#[rpc]
pub trait Rpc {
	/// Returns a protocol version
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> Result<String>;

	/// Adds two numbers and returns a result
	#[rpc(name = "add", alias("callAsyncMetaAlias"))]
	fn add(&self, a: u64, b: u64) -> Result<u64>;

	/// Performs asynchronous operation
	#[rpc(name = "callAsync")]
	fn call(&self, a: u64) -> BoxFuture<Result<String>>;
}

struct RpcImpl;

impl Rpc for RpcImpl {
	fn protocol_version(&self) -> Result<String> {
		Ok("version1".into())
	}

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn call(&self, _: u64) -> BoxFuture<Result<String>> {
		Box::pin(future::ready(Ok("OK".to_owned())))
	}
}

fn main() {
	let mut io = IoHandler::new();
	io.extend_with(RpcImpl.to_delegate());

	let (client, server) = local::connect::<gen_client::Client, _, _>(io);
	let fut = client.add(5, 6).map_ok(|res| println!("5 + 6 = {}", res));
	futures::executor::block_on(async move { futures::join!(fut, server) })
		.0
		.unwrap();
}
```
