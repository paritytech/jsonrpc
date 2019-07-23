use std::collections::BTreeMap;

use jsonrpc_core::futures::future::FutureResult;
use jsonrpc_core::types::params::Params;
use jsonrpc_core::{futures, Error, MetaIoHandler, Metadata, Result, Value};
use jsonrpc_derive::rpc;

#[derive(Clone)]
struct Meta(String);
impl Metadata for Meta {}

#[rpc]
pub trait Rpc<One> {
	type Metadata;

	/// Get One type.
	#[rpc(name = "getOne")]
	fn one(&self) -> Result<One>;

	/// Adds two numbers and returns a result.
	#[rpc(name = "add")]
	fn add(&self, a: u64, b: u64) -> Result<u64>;

	/// Multiplies two numbers. Second number is optional.
	#[rpc(name = "mul")]
	fn mul(&self, a: u64, b: Option<u64>) -> Result<u64>;

	/// Retrieves and debug prints the underlying `Params` object.
	#[rpc(name = "raw", raw_params)]
	fn raw(&self, params: Params) -> Result<String>;

	/// Performs an asynchronous operation.
	#[rpc(name = "callAsync")]
	fn call(&self, a: u64) -> FutureResult<String, Error>;

	/// Performs an asynchronous operation with meta.
	#[rpc(meta, name = "callAsyncMeta", alias("callAsyncMetaAlias"))]
	fn call_meta(&self, a: Self::Metadata, b: BTreeMap<String, Value>) -> FutureResult<String, Error>;

	/// Handles a notification.
	#[rpc(name = "notify")]
	fn notify(&self, a: u64);
}

struct RpcImpl;
impl Rpc<u64> for RpcImpl {
	type Metadata = Meta;

	fn one(&self) -> Result<u64> {
		Ok(100)
	}

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn mul(&self, a: u64, b: Option<u64>) -> Result<u64> {
		Ok(a * b.unwrap_or(1))
	}

	fn raw(&self, params: Params) -> Result<String> {
		Ok(format!("Got: {:?}", params))
	}

	fn call(&self, x: u64) -> FutureResult<String, Error> {
		futures::finished(format!("OK: {}", x))
	}

	fn call_meta(&self, meta: Self::Metadata, map: BTreeMap<String, Value>) -> FutureResult<String, Error> {
		futures::finished(format!("From: {}, got: {:?}", meta.0, map))
	}

	fn notify(&self, a: u64) {
		println!("Received `notify` with value: {}", a);
	}
}

fn main() {
	let mut io = MetaIoHandler::default();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate());

	let server =
		jsonrpc_tcp_server::ServerBuilder::with_meta_extractor(io, |context: &jsonrpc_tcp_server::RequestContext| {
			Meta(format!("{}", context.peer_addr))
		})
		.start(&"0.0.0.0:3030".parse().unwrap())
		.expect("Server must start with no issues");

	server.wait()
}
