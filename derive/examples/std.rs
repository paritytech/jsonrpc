//! A simple example
#![deny(missing_docs)]
use jsonrpc_core::futures::{self, future, TryFutureExt};
use jsonrpc_core::{BoxFuture, IoHandler, Result};
use jsonrpc_core_client::transports::local;
use jsonrpc_derive::rpc;

/// Rpc trait
#[rpc]
pub trait Rpc {
	/// Returns a protocol version.
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> Result<String>;

	/// Adds two numbers and returns a result.
	#[rpc(name = "add", alias("callAsyncMetaAlias"))]
	fn add(&self, a: u64, b: u64) -> Result<u64>;

	/// Performs asynchronous operation.
	#[rpc(name = "callAsync")]
	fn call(&self, a: u64) -> BoxFuture<Result<String>>;

	/// Handles a notification.
	#[rpc(name = "notify")]
	fn notify(&self, a: u64);
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

	fn notify(&self, a: u64) {
		println!("Received `notify` with value: {}", a);
	}
}

fn main() {
	let mut io = IoHandler::new();
	io.extend_with(RpcImpl.to_delegate());

	let (client, server) = local::connect::<RpcClient, _, _>(io);
	let fut = client.add(5, 6).map_ok(|res| println!("5 + 6 = {}", res));

	futures::executor::block_on(async move { futures::join!(fut, server) })
		.0
		.unwrap();
}
