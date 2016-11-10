extern crate jsonrpc_core;

use jsonrpc_core::*;

struct SayHello;
impl SyncMethodCommand for SayHello {
    fn execute(&self, _params: Params) -> Result<Value, Error> {
        Ok(Value::String("hello".to_string()))
    }
}

fn main() {
	let io = IoHandler::new();
	io.add_method("say_hello", SayHello);

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
}
