extern crate serde;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_derive;
#[macro_use]
extern crate serde_derive;

use jsonrpc_core::{IoHandler, Error, Result};
use jsonrpc_core::futures::future::{self, FutureResult};

// One is both parameter and a result so requires both Serialize and DeserializeOwned
// Two is only a parameter so only requires DeserializeOwned
// Three is only a result so only requires Serialize
#[rpc]
pub trait Rpc<One, Two, Three>
{
	/// Get One type.
	#[rpc(name = "getOne")]
	fn one(&self) -> Result<One>;

	/// Adds two numbers and returns a result
	#[rpc(name = "setTwo")]
	fn set_two(&self, Two) -> Result<()>;

	#[rpc(name = "getThree")]
	fn get_three(&self) -> Result<Three>;

	/// Performs asynchronous operation
	#[rpc(name = "beFancy")]
	fn call(&self, One) -> FutureResult<(One, u64), Error>;
}

struct RpcImpl;

#[derive(Serialize, Deserialize)]
struct InAndOut { foo: u64 }
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

	fn call(&self, num: InAndOut) -> FutureResult<(InAndOut, u64), Error> {
		::future::finished((InAndOut {foo: num.foo + 999}, num.foo))
	}
}

fn main() {
	let mut io = IoHandler::new();

	io.extend_with(Rpc::to_delegate(RpcImpl));
}

