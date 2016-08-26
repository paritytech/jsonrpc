//! method and notification commands executor

use std::sync::RwLock;
use std::collections::HashMap;
use async::{AsyncResult, Ready};
use super::{Params, Value, Error, ErrorCode};

/// Result of Method invocation.
pub enum MethodResult {
	/// Synchronous result
	Sync(Result<Value, Error>),
	/// Asynchronous result
	Async(AsyncResult),
}

/// Should be used to handle single synchronous method call.
pub trait SyncMethodCommand: Send + Sync {
	/// Execute synchronous method
	fn execute(&self, params: Params) -> Result<Value, Error>;
}

/// Default method command implementation for closure handling sync call.
impl<F> SyncMethodCommand for F where F: Fn(Params) -> Result<Value, Error>, F: Sync + Send {
	fn execute(&self, params: Params) -> Result<Value, Error> {
		self(params)
	}
}

/// Should be used to handle single asynchronous method call
pub trait AsyncMethodCommand: Send + Sync {
	/// Execute asynchronous method
	fn execute(&self, params: Params, ready: Ready);
}

impl<F> AsyncMethodCommand for F where F: Fn(Params, Ready), F: Sync + Send {
	fn execute(&self, params: Params, ready: Ready) {
		self(params, ready)
	}
}

/// Asynchronous command wrapper
pub struct AsyncMethod<F> where F: AsyncMethodCommand {
	command: F,
}

impl<F> AsyncMethod<F> where F: AsyncMethodCommand {
	/// Create new asynchronous command wrapper
	pub fn new(command: F) -> Self {
		AsyncMethod {
			command: command,
		}
	}
}

/// Should be used to handle single method call.
pub trait MethodCommand: Send + Sync {
	/// Execute this method and get result (sync / async)
	fn execute(&self, params: Params) -> MethodResult;
}

impl<F> MethodCommand for F where F: SyncMethodCommand {
	fn execute(&self, params: Params) -> MethodResult {
		MethodResult::Sync(self.execute(params))
	}
}

impl<F> MethodCommand for AsyncMethod<F> where F: AsyncMethodCommand {
	fn execute(&self, params: Params) -> MethodResult {
		let (res, ready) = AsyncResult::new();
		self.command.execute(params, ready);
		res.into()
	}
}

/// Should be used to handle single notification.
pub trait NotificationCommand: Send + Sync {
	/// Execute notification
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
	/// Creates new executor
	pub fn new() -> Self {
		Commander {
			methods: RwLock::new(HashMap::new()),
			notifications: RwLock::new(HashMap::new())
		}
	}

	/// Add supported method to this executor
	pub fn add_method<C>(&self, name: String, command: Box<C>) where C: MethodCommand + 'static {
		self.methods.write().unwrap().insert(name, command);
	}

	/// Add supported methods to this executor
	pub fn add_methods(&self, methods: HashMap<String, Box<MethodCommand>>) {
		self.methods.write().unwrap().extend(methods);
	}

	/// Add supported notification to this executor
	pub fn add_notification<C>(&self, name: String, command: Box<C>) where C: NotificationCommand + 'static {
		self.notifications.write().unwrap().insert(name, command);
	}

	/// Add supported notifications to this executor
	pub fn add_notifications(&self, notifications: HashMap<String, Box<NotificationCommand>>) {
		self.notifications.write().unwrap().extend(notifications);
	}

	/// Execute method identified by `name` with given `params`.
	pub fn execute_method(&self, name: String, params: Params) -> MethodResult {
		match self.methods.read().unwrap().get(&name) {
			Some(command) => command.execute(params),
			None => MethodResult::Sync(Err(Error::new(ErrorCode::MethodNotFound)))
		}
	}

	/// Execute notification identified by `name` with given `params`.
	pub fn execute_notification(&self, name: String, params: Params) {
		if let Some(command) = self.notifications.read().unwrap().get(&name) {
			command.execute(params)
		}
	}
}
