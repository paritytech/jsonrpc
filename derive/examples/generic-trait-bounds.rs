use serde::{Deserialize, Serialize};

use jsonrpc_core::{futures::future, BoxFuture, IoHandler, IoHandlerExtension, Result};
use jsonrpc_derive::rpc;

// One is both parameter and a result so requires both Serialize and DeserializeOwned
// Two is only a parameter so only requires DeserializeOwned
// Three is only a result so only requires Serialize
#[rpc(server)]
pub trait Rpc<One, Two, Three> {
	/// Get One type.
	#[rpc(name = "getOne")]
	fn one(&self) -> Result<One>;

	/// Adds two numbers and returns a result
	#[rpc(name = "setTwo")]
	fn set_two(&self, a: Two) -> Result<()>;

	#[rpc(name = "getThree")]
	fn get_three(&self) -> Result<Three>;

	/// Performs asynchronous operation
	#[rpc(name = "beFancy")]
	fn call(&self, a: One) -> BoxFuture<Result<(One, u64)>>;
}

struct RpcImpl;

#[derive(Serialize, Deserialize)]
struct InAndOut {
	foo: u64,
}
#[derive(Deserialize)]
struct In {}
#[derive(Serialize)]
struct Out {}

impl Rpc<InAndOut, In, Out> for RpcImpl {
	fn one(&self) -> Result<InAndOut> {
		Ok(InAndOut { foo: 1u64 })
	}

	fn set_two(&self, _x: In) -> Result<()> {
		Ok(())
	}

	fn get_three(&self) -> Result<Out> {
		Ok(Out {})
	}

	fn call(&self, num: InAndOut) -> BoxFuture<Result<(InAndOut, u64)>> {
		Box::pin(future::ready(Ok((InAndOut { foo: num.foo + 999 }, num.foo))))
	}
}

fn main() {
	let mut io = IoHandler::new();

	RpcImpl.to_delegate().augment(&mut io);
}
