use jsonrpc_core::{IoHandler, Response, Result};
use jsonrpc_derive::rpc;
use serde_json;

#[rpc]
pub trait Rpc {
	/// Multiplies two numbers. Second number is optional.
	#[rpc(name = "mul")]
	fn mul(&self, a: u64, b: Option<u64>) -> Result<u64>;

	/// Echos back the message, example of a single param trailing
	#[rpc(name = "echo")]
	fn echo(&self, a: Option<String>) -> Result<String>;

	/// Adds up to three numbers and returns a result
	#[rpc(name = "add_multi")]
	fn add_multi(&self, a: Option<u64>, b: Option<u64>, c: Option<u64>) -> Result<u64>;
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

	fn add_multi(&self, a: Option<u64>, b: Option<u64>, c: Option<u64>) -> Result<u64> {
		Ok(a.unwrap_or_default() + b.unwrap_or_default() + c.unwrap_or_default())
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
	assert_eq!(
		result,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 4,
		"id": 1
	}"#
		)
		.unwrap()
	);
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
	assert_eq!(
		result,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 2,
		"id": 1
	}"#
		)
		.unwrap()
	);
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
	assert_eq!(
		result1,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": "hello",
		"id": 1
	}"#
		)
		.unwrap()
	);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(
		result2,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": "",
		"id": 1
	}"#
		)
		.unwrap()
	);
}

#[test]
fn should_accept_multiple_trailing_params() {
	let mut io = IoHandler::new();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"add_multi","params":[]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"add_multi","params":[1]}"#;
	let req3 = r#"{"jsonrpc":"2.0","id":1,"method":"add_multi","params":[1, 2]}"#;
	let req4 = r#"{"jsonrpc":"2.0","id":1,"method":"add_multi","params":[1, 2, 3]}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);
	let res3 = io.handle_request_sync(req3);
	let res4 = io.handle_request_sync(req4);

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(
		result1,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 0,
		"id": 1
	}"#
		)
		.unwrap()
	);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(
		result2,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 1,
		"id": 1
	}"#
		)
		.unwrap()
	);

	let result3: Response = serde_json::from_str(&res3.unwrap()).unwrap();
	assert_eq!(
		result3,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 3,
		"id": 1
	}"#
		)
		.unwrap()
	);

	let result4: Response = serde_json::from_str(&res4.unwrap()).unwrap();
	assert_eq!(
		result4,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 6,
		"id": 1
	}"#
		)
		.unwrap()
	);
}
