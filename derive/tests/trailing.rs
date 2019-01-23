use serde_json;
use jsonrpc_derive::rpc;
use jsonrpc_core::{IoHandler, Response, Result};

#[rpc]
pub trait Rpc {
	/// Multiplies two numbers. Second number is optional.
	#[rpc(name = "mul")]
	fn mul(&self, _: u64, _: Option<u64>) -> Result<u64>;

	/// Echos back the message, example of a single param trailing
	#[rpc(name = "echo")]
	fn echo(&self, _: Option<String>) -> Result<String>;
}

#[derive(Default)]
struct RpcImpl;

impl Rpc for RpcImpl {
	fn mul(&self, a: u64, b: Option<u64>) -> Result<u64> {
		Ok(a * b.unwrap_or(1))
	}

	fn echo(&self, x: Option<String>) -> Result<String> {
		Ok(x.unwrap_or("".into()))
	}
}

#[test]
fn should_accept_trailing_param() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"mul","params":[2, 2]}"#;
	let res = io.handle_request_sync(req);

	// then
	let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
	assert_eq!(result, serde_json::from_str(r#"{
		"jsonrpc": "2.0",
		"result": 4,
		"id": 1
	}"#).unwrap());
}

#[test]
fn should_accept_missing_trailing_param() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req = r#"{"jsonrpc":"2.0","id":1,"method":"mul","params":[2]}"#;
	let res = io.handle_request_sync(req);

	// then
	let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
	assert_eq!(result, serde_json::from_str(r#"{
		"jsonrpc": "2.0",
		"result": 2,
		"id": 1
	}"#).unwrap());
}

#[test]
fn should_accept_single_trailing_param() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"echo","params":["hello"]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"echo","params":[]}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(result1, serde_json::from_str(r#"{
		"jsonrpc": "2.0",
		"result": "hello",
		"id": 1
	}"#).unwrap());

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(result2, serde_json::from_str(r#"{
		"jsonrpc": "2.0",
		"result": "",
		"id": 1
	}"#).unwrap());
}
