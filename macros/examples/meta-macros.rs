extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_macros;
extern crate jsonrpc_tcp_server;

use std::collections::BTreeMap;

use jsonrpc_core::{futures, BoxFuture, MetaIoHandler, Metadata, Error, Value};

#[derive(Clone, Default)]
struct Meta(String);
impl Metadata for Meta {}

build_rpc_trait! {
	pub trait Rpc {
		type Metadata;

		/// Adds two numbers and returns a result
		#[rpc(name = "add")]
		fn add(&self, u64, u64) -> Result<u64, Error>;

		/// Multiplies two numbers. Second number is optional.
		#[rpc(name = "mul")]
		fn mul(&self, u64, jsonrpc_macros::Trailing<u64>) -> Result<u64, Error>;

		/// Performs asynchronous operation
		#[rpc(async, name = "callAsync")]
		fn call(&self, u64) -> BoxFuture<String, Error>;

		/// Performs asynchronous operation with meta
		#[rpc(meta, name = "callAsyncMeta", alias = [ "callAsyncMetaAlias", ])]
		fn call_meta(&self, Self::Metadata, BTreeMap<String, Value>) -> BoxFuture<String, Error>;
	}
}

struct RpcImpl;
impl Rpc for RpcImpl {
	type Metadata = Meta;

	fn add(&self, a: u64, b: u64) -> Result<u64, Error> {
		Ok(a + b)
	}

	fn mul(&self, a: u64, b: jsonrpc_macros::Trailing<u64>) -> Result<u64, Error> {
		Ok(a * b.unwrap_or(1))
	}

	fn call(&self, x: u64) -> BoxFuture<String, Error> {
		Box::new(futures::finished(format!("OK: {}", x)))
	}

	fn call_meta(&self, meta: Self::Metadata, map: BTreeMap<String, Value>) -> BoxFuture<String, Error> {
		Box::new(futures::finished(format!("From: {}, got: {:?}", meta.0, map)))
	}
}


fn main() {
	let mut io = MetaIoHandler::default();
	let rpc = RpcImpl;

	io.extend_with(rpc.to_delegate());

	let server = jsonrpc_tcp_server::ServerBuilder::new(io)
		.session_meta_extractor(|context: &jsonrpc_tcp_server::RequestContext| {
			Meta(format!("{}", context.peer_addr))
		})
		.start(&"0.0.0.0:3030".parse().unwrap())
		.expect("Server must start with no issues");

	server.wait()
}
