use jsonrpc_core::*;

fn main() {
	futures_executor::block_on(async {
		let mut io = IoHandler::new();

		io.add_method("say_hello", |_: Params| async {
			Ok(Value::String("Hello World!".to_owned()))
		});

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"Hello World!","id":1}"#;

		assert_eq!(io.handle_request(request).await, Some(response.to_owned()));
	});
}
