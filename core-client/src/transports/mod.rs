//! Client transport implementations


use jsonrpc_core::{Call, Id, MethodCall, Version};
use crate::RpcMessage;

pub mod local;
pub mod http;
pub mod duplex;

pub use duplex::Duplex;

/// Creates JSON-RPC requests
pub struct RequestBuilder {
	id: u64,
}

impl RequestBuilder {
	/// Create a new RequestBuilder
	pub fn new() -> Self {
		RequestBuilder {
			id: 0
		}
	}

	fn next_id(&mut self) -> Id {
		let id = self.id;
		self.id = id + 1;
		Id::Num(id)
	}

	/// Build a single request with the next available id
	pub fn single_request(&mut self, msg: &RpcMessage) -> (Id, String) {
		let id = self.next_id();
		let request = jsonrpc_core::Request::Single(Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method: msg.method.clone(),
			params: msg.params.clone(),
			id: id.clone(),
		}));
		(id, serde_json::to_string(&request).expect("Request serialization is infallible; qed"))
	}
}
