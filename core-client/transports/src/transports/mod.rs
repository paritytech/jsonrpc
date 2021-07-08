//! Client transport implementations

use jsonrpc_core::{Call, Error, Id, MethodCall, Notification, Params, Version};
use jsonrpc_pubsub::SubscriptionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CallMessage, NotifyMessage, RpcError};

pub mod duplex;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "ipc")]
pub mod ipc;
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

	fn subscribe_request(&mut self, subscribe: String, subscribe_params: Params) -> (Id, String) {
		self.single_request(subscribe, subscribe_params)
	}

	fn unsubscribe_request(&mut self, unsubscribe: String, sid: SubscriptionId) -> (Id, String) {
		self.single_request(unsubscribe, Params::Array(vec![Value::from(sid)]))
	}

	fn notification(&mut self, msg: &NotifyMessage) -> String {
		let request = jsonrpc_core::Request::Single(Call::Notification(Notification {
			jsonrpc: Some(Version::V2),
			method: msg.method.clone(),
			params: msg.params.clone(),
		}));
		serde_json::to_string(&request).expect("Request serialization is infallible; qed")
	}
}

/// Parse raw string into a single JSON value, together with the request Id.
///
/// This method will attempt to parse a JSON-RPC response object (either `Failure` or `Success`)
/// and a `Notification` (for Subscriptions).
/// Note that if you have more specific expectations about the returned type and don't want
/// to handle all of them it might be best to deserialize on your own.
pub fn parse_response(
	response: &str,
) -> Result<(Id, Result<Value, RpcError>, Option<String>, Option<SubscriptionId>), RpcError> {
	jsonrpc_core::serde_from_str::<ClientResponse>(response)
		.map_err(|e| RpcError::ParseError(e.to_string(), Box::new(e)))
		.map(|response| {
			let id = response.id().unwrap_or(Id::Null);
			let sid = response.subscription_id();
			let method = response.method();
			let value: Result<Value, Error> = response.into();
			let result = value.map_err(RpcError::JsonRpcError);
			(id, result, method, sid)
		})
}

/// A type representing all possible values sent from the server to the client.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum ClientResponse {
	/// A regular JSON-RPC request output (single response).
	Output(jsonrpc_core::Output),
	/// A notification.
	Notification(jsonrpc_core::Notification),
}

impl ClientResponse {
	/// Get the id of the response (if any).
	pub fn id(&self) -> Option<Id> {
		match *self {
			ClientResponse::Output(ref output) => Some(output.id().clone()),
			ClientResponse::Notification(_) => None,
		}
	}

	/// Get the method name if the output is a notification.
	pub fn method(&self) -> Option<String> {
		match *self {
			ClientResponse::Notification(ref n) => Some(n.method.to_owned()),
			ClientResponse::Output(_) => None,
		}
	}

	/// Parses the response into a subscription id.
	pub fn subscription_id(&self) -> Option<SubscriptionId> {
		match *self {
			ClientResponse::Notification(ref n) => match &n.params {
				jsonrpc_core::Params::Map(map) => match map.get("subscription") {
					Some(value) => SubscriptionId::parse_value(value),
					None => None,
				},
				_ => None,
			},
			_ => None,
		}
	}
}

impl From<ClientResponse> for Result<Value, Error> {
	fn from(res: ClientResponse) -> Self {
		match res {
			ClientResponse::Output(output) => output.into(),
			ClientResponse::Notification(n) => match &n.params {
				Params::Map(map) => {
					let subscription = map.get("subscription");
					let result = map.get("result");
					let error = map.get("error");

					match (subscription, result, error) {
						(Some(_), Some(result), _) => Ok(result.to_owned()),
						(Some(_), _, Some(error)) => {
							let error = serde_json::from_value::<Error>(error.to_owned())
								.ok()
								.unwrap_or_else(Error::parse_error);
							Err(error)
						}
						_ => Ok(n.params.into()),
					}
				}
				_ => Ok(n.params.into()),
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use jsonrpc_core::{Failure, Notification, Output, Params, Success, Value, Version};

	#[test]
	fn notification_deserialize() {
		let dsr = r#"{"jsonrpc":"2.0","method":"hello","params":[10]}"#;

		let deserialized: ClientResponse = jsonrpc_core::serde_from_str(dsr).unwrap();
		assert_eq!(
			deserialized,
			ClientResponse::Notification(Notification {
				jsonrpc: Some(Version::V2),
				method: "hello".into(),
				params: Params::Array(vec![Value::from(10)]),
			})
		);
	}

	#[test]
	fn success_deserialize() {
		let dsr = r#"{"jsonrpc":"2.0","result":1,"id":1}"#;

		let deserialized: ClientResponse = jsonrpc_core::serde_from_str(dsr).unwrap();
		assert_eq!(
			deserialized,
			ClientResponse::Output(Output::Success(Success {
				jsonrpc: Some(Version::V2),
				id: Id::Num(1),
				result: 1.into(),
			}))
		);
	}

	#[test]
	fn failure_output_deserialize() {
		let dfo = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":1}"#;

		let deserialized: ClientResponse = jsonrpc_core::serde_from_str(dfo).unwrap();
		assert_eq!(
			deserialized,
			ClientResponse::Output(Output::Failure(Failure {
				jsonrpc: Some(Version::V2),
				error: Error::parse_error(),
				id: Id::Num(1)
			}))
		);
	}
}
