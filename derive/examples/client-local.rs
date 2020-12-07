use jsonrpc_core::{
	futures::{self, FutureExt},
	BoxFuture, IoHandler, Result,
};
use jsonrpc_core_client::transports::local;
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
		Box::pin(futures::future::ready(Ok("OK".to_owned())))
	}
}

fn main() {
	futures::executor::block_on(async {
		let mut io = IoHandler::new();
		io.extend_with(RpcImpl.to_delegate());
		println!("Starting local server");
		let (client, server) = local::connect(io);
		let client = use_client(client).fuse();
		let server = server.fuse();

		futures::pin_mut!(client);
		futures::pin_mut!(server);

		futures::select! {
			_server = server => {},
			_client = client => {},
		}
	});
}

async fn use_client(client: RpcClient) {
	let res = client.add(5, 6).await.unwrap();
	println!("5 + 6 = {}", res);
}
