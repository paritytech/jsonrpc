use jsonrpc_core::types::params::Params;
use jsonrpc_core::{IoHandler, Response};
use jsonrpc_derive::rpc;
use serde;
use serde_json;

pub enum MyError {}
impl From<MyError> for jsonrpc_core::Error {
	fn from(_e: MyError) -> Self {
		unreachable!()
	}
}

type Result<T> = ::std::result::Result<T, MyError>;

#[rpc]
pub trait Rpc {
	type Metadata;

	/// Returns a protocol version.
	#[rpc(name = "protocolVersion")]
	fn protocol_version(&self) -> Result<String>;

	/// Negates number and returns a result.
	#[rpc(name = "neg")]
	fn neg(&self, a: i64) -> Result<i64>;

	/// Adds two numbers and returns a result.
	#[rpc(name = "add", alias("add_alias1", "add_alias2"))]
	fn add(&self, a: u64, b: u64) -> Result<u64>;

	/// Retrieves and debug prints the underlying `Params` object.
	#[rpc(name = "raw", params = "raw")]
	fn raw(&self, params: Params) -> Result<String>;

	/// Adds two numbers and returns a result.
	#[rpc(name = "named_add", params = "named")]
	fn named_add(&self, a: u64, b: u64) -> Result<u64>;

	/// Adds one or two numbers and returns a result.
	#[rpc(name = "option_named_add", params = "named")]
	fn option_named_add(&self, a: u64, b: Option<u64>) -> Result<u64>;

	/// Adds two numbers and returns a result.
	#[rpc(meta, name = "meta_named_add", params = "named")]
	fn meta_named_add(&self, meta: Self::Metadata, a: u64, b: u64) -> Result<u64>;

	/// Handles a notification.
	#[rpc(name = "notify")]
	fn notify(&self, a: u64);
}

#[derive(Default)]
struct RpcImpl;

impl Rpc for RpcImpl {
	type Metadata = Metadata;

	fn protocol_version(&self) -> Result<String> {
		Ok("version1".into())
	}

	fn neg(&self, a: i64) -> Result<i64> {
		Ok(-a)
	}

	fn add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn raw(&self, _params: Params) -> Result<String> {
		Ok("OK".into())
	}

	fn named_add(&self, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn option_named_add(&self, a: u64, b: Option<u64>) -> Result<u64> {
		Ok(a + b.unwrap_or_default())
	}

	fn meta_named_add(&self, _meta: Self::Metadata, a: u64, b: u64) -> Result<u64> {
		Ok(a + b)
	}

	fn notify(&self, a: u64) {
		println!("Received `notify` with value: {}", a);
	}
}

#[derive(Clone, Default)]
struct Metadata;
impl jsonrpc_core::Metadata for Metadata {}

#[test]
fn should_accept_empty_array_as_no_params() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"protocolVersion","params":[]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"protocolVersion","params":null}"#;
	let req3 = r#"{"jsonrpc":"2.0","id":1,"method":"protocolVersion"}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);
	let res3 = io.handle_request_sync(req3);
	let expected = r#"{
		"jsonrpc": "2.0",
		"result": "version1",
		"id": 1
	}"#;
	let expected: Response = serde_json::from_str(expected).unwrap();

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(expected, result1);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(expected, result2);

	let result3: Response = serde_json::from_str(&res3.unwrap()).unwrap();
	assert_eq!(expected, result3);
}

#[test]
fn should_accept_single_param() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"neg","params":[1]}"#;

	let res1 = io.handle_request_sync(req1);

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(
		result1,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": -1,
		"id": 1
	}"#
		)
		.unwrap()
	);
}

#[test]
fn should_accept_multiple_params() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"add","params":[1, 2]}"#;

	let res1 = io.handle_request_sync(req1);

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(
		result1,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 3,
		"id": 1
	}"#
		)
		.unwrap()
	);
}

#[test]
fn should_use_method_name_aliases() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"add_alias1","params":[1, 2]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"add_alias2","params":[1, 2]}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(
		result1,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"result": 3,
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
		"result": 3,
		"id": 1
	}"#
		)
		.unwrap()
	);
}

#[test]
fn should_accept_any_raw_params() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"raw","params":[1, 2]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"raw","params":{"foo":"bar"}}"#;
	let req3 = r#"{"jsonrpc":"2.0","id":1,"method":"raw","params":null}"#;
	let req4 = r#"{"jsonrpc":"2.0","id":1,"method":"raw"}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);
	let res3 = io.handle_request_sync(req3);
	let res4 = io.handle_request_sync(req4);
	let expected = r#"{
		"jsonrpc": "2.0",
		"result": "OK",
		"id": 1
	}"#;
	let expected: Response = serde_json::from_str(expected).unwrap();

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(expected, result1);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(expected, result2);

	let result3: Response = serde_json::from_str(&res3.unwrap()).unwrap();
	assert_eq!(expected, result3);

	let result4: Response = serde_json::from_str(&res4.unwrap()).unwrap();
	assert_eq!(expected, result4);
}

#[test]
fn should_accept_named_params() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"named_add","params":{"a":1,"b":2}}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"named_add","params":{"b":2,"a":1}}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);

	let expected = r#"{
	    "jsonrpc": "2.0",
		"result": 3,
		"id": 1
    }"#;
	let expected: Response = serde_json::from_str(expected).unwrap();

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(expected, result1);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(expected, result2);
}

#[test]
fn should_accept_option_named_params() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"option_named_add","params":{"a":1,"b":2}}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"option_named_add","params":{"a":3}}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);

	let expected = r#"{
	    "jsonrpc": "2.0",
		"result": 3,
		"id": 1
    }"#;
	let expected: Response = serde_json::from_str(expected).unwrap();

	// then
	let result1: Response = serde_json::from_str(&res1.unwrap()).unwrap();
	assert_eq!(expected, result1);

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(expected, result2);
}

#[test]
fn should_accept_only_notifications() {
	let mut io = IoHandler::default();
	let rpc = RpcImpl::default();
	io.extend_with(rpc.to_delegate());

	// when
	let req1 = r#"{"jsonrpc":"2.0","method":"notify","params":[1]}"#;
	let req2 = r#"{"jsonrpc":"2.0","id":1,"method":"notify","params":[1]}"#;

	let res1 = io.handle_request_sync(req1);
	let res2 = io.handle_request_sync(req2);

	// then
	assert!(res1.is_none());

	let result2: Response = serde_json::from_str(&res2.unwrap()).unwrap();
	assert_eq!(
		result2,
		serde_json::from_str(
			r#"{
		"jsonrpc": "2.0",
		"error": {
			"code": -32601,
			"message": "Method not found"
		},
		"id":1
	}"#
		)
		.unwrap()
	);
}
