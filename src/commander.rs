//! method and notification commands executor

use std::sync::RwLock;
use std::collections::HashMap;
use super::{Params, Value, Error, ErrorCode};

/// Should be used to handle single method call.
pub trait MethodCommand: Send + Sync {
	fn execute(&self, params: Params) -> Result<Value, Error>;
}

/// Default method command implementation for closure.
impl<F> MethodCommand for F where F: Fn(Params) -> Result<Value, Error>, F: Sync + Send {
	fn execute(&self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

/// Should be used to handle single notification.
pub trait NotificationCommand: Send + Sync {
	fn execute(&self, params: Params);
}

/// Default notification command implementation for closure.
impl<F> NotificationCommand for F where F: Fn(Params), F: Sync + Send {
	fn execute(&self, params: Params) {
		self(params)
	}
}

/// Commands executor.
pub struct Commander {
	methods: RwLock<HashMap<String, Box<MethodCommand>>>,
	notifications: RwLock<HashMap<String, Box<NotificationCommand>>>
}

impl Commander {
	pub fn new() -> Self {
		Commander {
			methods: RwLock::new(HashMap::new()),
			notifications: RwLock::new(HashMap::new())
		}
	}

	pub fn add_method<C>(&self, name: String, command: Box<C>) where C: MethodCommand + 'static {
		self.methods.write().unwrap().insert(name, command);
	}

	pub fn add_methods(&self, methods: HashMap<String, Box<MethodCommand>>) {
		self.methods.write().unwrap().extend(methods);
	}

	pub fn add_notification<C>(&self, name: String, command: Box<C>) where C: NotificationCommand + 'static {
		self.notifications.write().unwrap().insert(name, command);
	}

	pub fn add_notifications(&self, notifications: HashMap<String, Box<NotificationCommand>>) {
		self.notifications.write().unwrap().extend(notifications);
	}

	pub fn execute_method(&self, name: String, params: Params) -> Result<Value, Error> {
		match self.methods.read().unwrap().get(&name) {
			Some(command) => command.execute(params),
			None => Err(Error::new(ErrorCode::MethodNotFound))
		}
	}

	pub fn execute_notification(&self, name: String, params: Params) {
		if let Some(command) = self.notifications.read().unwrap().get(&name) {
			command.execute(params)
		}
	}
}
