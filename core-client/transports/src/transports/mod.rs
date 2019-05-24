//! Client transport implementations

use jsonrpc_core::{Call, Error, Id, MethodCall, Output, Params, Response, Version};
use jsonrpc_pubsub::SubscriptionId;
use serde_json::Value;

use crate::{CallMessage, RpcError, SubscribeMessage};

pub mod duplex;
#[cfg(feature = "http")]
pub mod http;
pub mod local;
#[cfg(feature = "ws")]
pub mod ws;

pub use duplex::duplex;

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
	fn single_request(&mut self, method: String, params: Params) -> (Id, String) {
		let id = self.next_id();
		let request = jsonrpc_core::Request::Single(Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method,
			params,
			id: id.clone(),
		}));
		(
			id,
			serde_json::to_string(&request).expect("Request serialization is infallible; qed"),
		)
	}

	fn call_request(&mut self, msg: &CallMessage) -> (Id, String) {
		self.single_request(msg.method.clone(), msg.params.clone())
	}

	fn subscribe_request(&mut self, msg: &SubscribeMessage) -> (Id, String) {
		self.single_request(msg.subscribe_method.clone(), msg.subscribe_params.clone())
	}

	fn unsubscribe_request(&mut self, unsubscribe: String, sid: SubscriptionId) -> (Id, String) {
		self.single_request(unsubscribe, Params::Array(vec![Value::from(sid)]))
	}
}

/// Parse raw string into JSON values, together with the request Id
pub fn parse_response(response: &str) -> Result<Vec<(Id, Result<Value, RpcError>, Option<String>)>, RpcError> {
	serde_json::from_str::<Response>(&response)
		.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
		.map(|response| {
			let outputs: Vec<Output> = match response {
				Response::Single(output) => vec![output],
				Response::Batch(outputs) => outputs,
			};
			outputs
				.into_iter()
				.map(|output| {
					let id = output.id().clone();
					let method = output.method();
					let value: Result<Value, Error> = output.into();
					let result = value.map_err(RpcError::JsonRpcError);
					(id, result, method)
				})
				.collect::<Vec<_>>()
		})
}
