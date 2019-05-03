//! Client transport implementations

use jsonrpc_core::{Call, Error, Id, MethodCall, Output, Response, Version};
use serde_json::Value;

use crate::{RpcError, RpcMessage};

pub mod duplex;
pub mod http;
pub mod local;
pub mod ws;

pub use duplex::Duplex;

/// Creates JSON-RPC requests
pub struct RequestBuilder {
	id: u64,
}

impl RequestBuilder {
	/// Create a new RequestBuilder
	pub fn new() -> Self {
		RequestBuilder { id: 0 }
	}

	fn next_id(&mut self) -> Id {
		let id = self.id;
		self.id = id + 1;
		Id::Num(id)
	}

	/// Build a single request with the next available id
	fn single_request(&mut self, msg: &RpcMessage) -> (Id, String) {
		let id = self.next_id();
		let request = jsonrpc_core::Request::Single(Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method: msg.method.clone(),
			params: msg.params.clone(),
			id: id.clone(),
		}));
		(
			id,
			serde_json::to_string(&request).expect("Request serialization is infallible; qed"),
		)
	}
}

/// Parse raw string into JSON values, together with the request Id
pub fn parse_response(response: &str) -> Result<Vec<(Id, Result<Value, RpcError>)>, RpcError> {
	serde_json::from_str::<Response>(&response)
		.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
		.map(|response| {
			let outputs: Vec<Output> = match response {
				Response::Single(output) => vec![output],
				Response::Batch(outputs) => outputs,
			};
			outputs
				.iter()
				.cloned()
				.map(|output| {
					let id = output.id().clone();
					let value: Result<Value, Error> = output.into();
					let result = value.map_err(RpcError::JsonRpcError);
					(id, result)
				})
				.collect::<Vec<_>>()
		})
}
