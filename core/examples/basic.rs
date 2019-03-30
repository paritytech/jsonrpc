use jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::new();

	io.add_method("say_hello", |_: Params| Ok(Value::String("Hello World!".to_owned())));

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}
