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
	pub fn add_method<C>(&self, name: String, command: C) where C: MethodCommand + 'static {
		self.commander.add_method(name, command)
	}

	/// Adds supported notification
	pub fn add_notification<C>(&self, name: String, command: C) where C: NotificationCommand + 'static {
		self.commander.add_notification(name, command)
	}

	/// Adds supported subscription
	pub fn add_subscription<C>(&self, subscribe: String, unsubscribe: String, command: C) where C: SubscriptionCommand + 'static {
		self.commander.add_subscription(subscribe, unsubscribe, command)
	}

	/// Adds a batch of supported methods
	pub fn add_methods(&self, methods: HashMap<String, Box<MethodCommand>>) {
		self.commander.add_methods(methods);
	}

	/// Adds a batch of supported notifications
	pub fn add_notifications(&self, notifications: HashMap<String, Box<NotificationCommand>>) {
		self.commander.add_notifications(notifications);
	}

	/// Adds a batch of supported subscriptions
	pub fn add_subscriptions(&self, subscriptions: HashMap<(String, String), Box<SubscriptionCommand>>) {
		self.commander.add_subscriptions(subscriptions);
	}

	/// Handle single request
	/// `Some(response)` is returned in case that request is a method call.
	/// `None` is returned in case of notifications and empty batches.
	pub fn handle_request<A: 'static>(&self, request: Request, handler: Handler<A, Option<Response>>, session: Option<Session>) {
		match request {
			Request::Single(call) => {
				self.handle_call(call, handler.map(|output: Option<Output>| output.map(Response::Single)), session)
			},
			Request::Batch(calls) => {
				let sub_handlers = handler.split_map(calls.len(), |responses| {
					let outs: Vec<Output> = responses.into_iter().filter_map(|v| v).collect();
					match outs.len() {
						0 => None,
						_ => Some(Response::Batch(outs))
					}
				});
				for (call, sub_handler) in calls.into_iter().zip(sub_handlers) {
					self.handle_call(call, sub_handler, session.clone());
				}
			}
		}
	}

	fn handle_call<A: 'static>(&self, call: Call, handler: Handler<A, Option<Output>>, session: Option<Session>) {
		match call {
			Call::MethodCall(method) => {
				self.handle_method_call(method, handler, session)
			},
			Call::Notification(notification) => {
				self.handle_notification(notification);
				handler.send(None)
			},
			Call::Invalid => handler.send(Some(Output::Failure(Failure {
				id: Id::Null,
				jsonrpc: Version::V2,
				error: Error::new(ErrorCode::InvalidRequest)
			}))),
		}
	}

	fn handle_method_call<A: 'static>(&self, method: MethodCall, handler: Handler<A, Option<Output>>, session: Option<Session>) {
		let params = method.params.unwrap_or(Params::None);
		let id = method.id;
		let jsonrpc = method.jsonrpc;

		self.commander.execute_method(method.method, params, handler.map(move |result| {
			Some(Output::from(result, id.clone(), jsonrpc.clone()))
		}), session)
	}

	fn handle_notification(&self, notification: Notification) {
		let params = notification.params.unwrap_or(Params::None);
		self.commander.execute_notification(notification.method, params)
	}
}
