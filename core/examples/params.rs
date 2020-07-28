use jsonrpc_core::*;
use serde_derive::Deserialize;

#[derive(Deserialize)]
struct HelloParams {
	name: String,
}

fn main() {
	let mut io = IoHandler::new();

	io.add_method("say_hello", |params: Params| async move {
		let parsed: HelloParams = params.parse().unwrap();
		Ok(Value::String(format!("hello, {}", parsed.name)))
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": { "name": "world" }, "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello, world","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}
