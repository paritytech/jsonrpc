extern crate jsonrpc_core;

use jsonrpc_core::*;

struct SayHello;
impl MethodCommand for SayHello {
    fn execute(&self, _params: Params, ready: Ready) {
        ready.ready(Ok(Value::String("hello".to_string())))
    }
}

fn main() {
	let io = IoHandler::new();
	io.add_async_method("say_hello", SayHello);

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

	io.handle_request(request, move |res| {
		assert_eq!(res, Some(response.to_string()));
	});
}

