use crate::Response;
use jsonrpc_core::serde_json;

pub trait ResponseMiddleware: Send + Sync + 'static {
	fn on_response(&self, response: jsonrpc_core::types::response::Response) -> Response;
}

#[derive(Default)]
pub struct NoopResponseMiddleware;
impl ResponseMiddleware for NoopResponseMiddleware {
	fn on_response(&self, response: jsonrpc_core::types::response::Response) -> Response {
		Response::ok(format!("{}\n", write_response(response)))
	}
}

pub fn write_response(response: jsonrpc_core::types::response::Response) -> String {
	serde_json::to_string(&response).unwrap_or_default()
}
