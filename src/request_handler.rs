//! jsonrpc server request handler
use std::collections::HashMap;
use super::*;

/// Requests handler - maps `Commander` outputs into well-formed JSONRPC `Responses`
pub struct RequestHandler {
	commander: Commander
}

impl Default for RequestHandler {
	fn default() -> Self {
		RequestHandler::new()
	}
}

impl RequestHandler {
	/// Creates new `RequestHandler`
	pub fn new() -> Self {
		RequestHandler {
			commander: Commander::new()
		}
	}

	/// Adds supported method
	pub fn add_method<C>(&self, name: String, command: Box<C>) where C: MethodCommand + 'static {
		self.commander.add_method(name, command)
	}

	/// Adds supported methods
	pub fn add_methods(&self, methods: HashMap<String, Box<MethodCommand>>) {
		self.commander.add_methods(methods);
	}

	/// Adds supported notification
	pub fn add_notification<C>(&self, name: String, command: Box<C>) where C: NotificationCommand + 'static {
		self.commander.add_notification(name, command)
	}

	/// Adds supported notifications
	pub fn add_notifications(&self, notifications: HashMap<String, Box<NotificationCommand>>) {
		self.commander.add_notifications(notifications);
	}

	/// Handle single request
	/// `Some(response)` is returned in case that request is a method call.
	/// `None` is returned in case of notifications and empty batches.
	pub fn handle_request(&self, request: Request) -> Option<Response> {
		match request {
			Request::Single(call) => self.handle_call(call).map(Response::Single),
			Request::Batch(calls) => {
				let outs: Vec<Output> = calls.into_iter().filter_map(|call| self.handle_call(call)).collect();
				match outs.len() {
					0 => None,
					_ => Some(Response::Batch(outs))
				}
			}
		}
	}

	fn handle_call(&self, call: Call) -> Option<Output> {
		match call {
			Call::MethodCall(method) => Some(self.handle_method_call(method)),
			Call::Notification(notification) => {
				self.handle_notification(notification);
				None
			},
			Call::Invalid(id) => Some(Output::Sync(SyncOutput::Failure(Failure {
				id: id,
				jsonrpc: Version::V2,
				error: Error::new(ErrorCode::InvalidRequest)
			})))
		}
	}

	fn handle_method_call(&self, method: MethodCall) -> Output {
		let params = match method.params {
			Some(p) => p,
			None => Params::None
		};

		match self.commander.execute_method(method.method, params) {
			MethodResult::Sync(res) => Output::Sync(SyncOutput::from(res, method.id, method.jsonrpc)),
			MethodResult::Async(result) => Output::Async(AsyncOutput::from(result, method.id, method.jsonrpc)),
		}
	}

	fn handle_notification(&self, notification: Notification) {
		let params = match notification.params {
			Some(p) => p,
			None => Params::None
		};

		self.commander.execute_notification(notification.method, params)
	}
}
