use jsonrpc_core::*;

fn main() {
	let mut io = IoHandler::new();

	io.add_method("say_hello", |_params: Params| {
    let parsed: Value = _params.parse().unwrap();
		Ok(Value::String(format!("{} {}","hello".to_string(), parsed["name"].to_string())))
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": { "name": "world" }, "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello \"world\"","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}
