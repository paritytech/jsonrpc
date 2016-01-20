//! method and notification commands executor

use std::collections::HashMap;
use super::{Params, Value, Error, ErrorCode};

/// Should be used to handle single method call.
pub trait MethodCommand: Send + Sync {
	fn execute(&mut self, params: Params) -> Result<Value, Error>;
}

/// Default method command implementation for closure.
impl<F> MethodCommand for F where F: Fn(Params) -> Result<Value, Error>, F: Sync + Send {
	fn execute(&mut self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

/// Should be used to handle single notification.
pub trait NotificationCommand: Send + Sync {
	fn execute(&mut self, params: Params);
}

/// Default notification command implementation for closure.
impl<F> NotificationCommand for F where F: Fn(Params), F: Sync + Send {
	fn execute(&mut self, params: Params) {
		self(params)
	}
}

/// Commands executor.
pub struct Commander {
	methods: HashMap<String, Box<MethodCommand>>,
	notifications: HashMap<String, Box<NotificationCommand>>
}

impl Commander {
	pub fn new() -> Self {
		Commander {
			methods: HashMap::new(),
			notifications: HashMap::new()
		}
	}

	pub fn add_method<C>(&mut self, name: String, command: Box<C>) where C: MethodCommand + 'static {
		self.methods.insert(name, command);
	}

	pub fn add_methods(&mut self, methods: HashMap<String, Box<MethodCommand>>) {
		self.methods.extend(methods);
	}

	pub fn add_notification<C>(&mut self, name: String, command: Box<C>) where C: NotificationCommand + 'static {
		self.notifications.insert(name, command);
	}

	pub fn add_notifications(&mut self, notifications: HashMap<String, Box<NotificationCommand>>) {
		self.notifications.extend(notifications);
	}

	pub fn execute_method(&mut self, name: String, params: Params) -> Result<Value, Error> {
		match self.methods.get_mut(&name) {
			Some(command) => command.execute(params),
			None => Err(Error::new(ErrorCode::MethodNotFound))
		}
	}

	pub fn execute_notification(&mut self, name: String, params: Params) {
		if let Some(command) = self.notifications.get_mut(&name) {
			command.execute(params)
		}
	}
}
