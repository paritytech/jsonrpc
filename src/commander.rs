//! method and notification commands executor

use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::RwLock;

use control::{Subscription, Ready, Data, Session, Handler, ResponseHandler};
use error::{Error, ErrorCode};
use params::Params;
use response::SubscriptionOutput;

/// Result of Method invocation.
pub enum Method {
	/// Method Call
	Call(Box<MethodCommand>),
	/// Notification
	Notify(Box<NotificationCommand>),
	/// New subscription
	Subscribe(Arc<Box<SubscriptionCommand>>, String),
	/// Close subscription
	Unsubscribe(Arc<Box<SubscriptionCommand>>),
}

/// Should be used to handle single synchronous method call.
pub trait SyncMethodCommand: Send + Sync {
	/// Execute synchronous method
	fn execute(&self, params: Params) -> Data;
}

/// Default method command implementation for closure handling sync call.
impl<F> SyncMethodCommand for F where F: Fn(Params) -> Data + Sync + Send {
	fn execute(&self, params: Params) -> Data {
		self(params)
	}
}

/// Wrapper type for `SyncMethodCommand`
pub struct SyncMethod<C> {
	/// Synchronous command to execute
	pub command: C,
}

impl<C: SyncMethodCommand> MethodCommand for SyncMethod<C> {
	fn execute(&self, params: Params, ready: Ready) {
		ready.ready(self.command.execute(params));
	}
}

/// Should be used to handle single asynchronous method call
pub trait MethodCommand: Send + Sync {
	/// Execute asynchronous method
	fn execute(&self, params: Params, ready: Ready);
}

impl<F> MethodCommand for F where F: Fn(Params, Ready) + Sync + Send {
	fn execute(&self, params: Params, ready: Ready) {
		self(params, ready)
	}
}

/// Should be used to handle single notification.
pub trait NotificationCommand: Send + Sync {
	/// Execute notification
	fn execute(&self, params: Params);
}

/// Default notification command implementation for closure.
impl<F> NotificationCommand for F where F: Fn(Params) + Sync + Send {
	fn execute(&self, params: Params) {
		self(params)
	}
}

/// Should be used to handle subscriptions
pub trait SubscriptionCommand: Send + Sync {
	/// Executes subscription
	fn execute(&self, subscription: Subscription);
}

/// Default subscription command implementation for closure.
impl<F> SubscriptionCommand for F where F: Fn(Subscription) + Sync + Send {
	fn execute(&self, subscription: Subscription) {
		self(subscription)
	}
}

/// Commands executor.
pub struct Commander {
	methods: RwLock<HashMap<String, Method>>,
}

impl Commander {
	/// Creates new executor
	pub fn new() -> Self {
		Commander {
			methods: RwLock::new(HashMap::new()),
		}
	}

	/// Add supported method to this executor
	pub fn add_method<C>(&self, name: String, command: C) where C: MethodCommand + 'static {
		self.methods.write().insert(name, Method::Call(Box::new(command)));
	}

	/// Add supported notification to this executor
	pub fn add_notification<C>(&self, name: String, command: C) where C: NotificationCommand + 'static {
		self.methods.write().insert(name, Method::Notify(Box::new(command)));
	}

	/// Add supported notification to this executor
	pub fn add_subscription<C>(&self, subscribe: String, subscription: String, unsubscribe: String, command: C) where C: SubscriptionCommand + 'static {
		let command = Arc::new(Box::new(command) as Box<SubscriptionCommand>);
		let mut methods = self.methods.write();
		methods.insert(subscribe, Method::Subscribe(command.clone(), subscription));
		methods.insert(unsubscribe, Method::Unsubscribe(command));
	}

	/// Add supported methods to this executor
	pub fn add_methods(&self, methods: HashMap<String, Box<MethodCommand>>) {
		let methods: HashMap<_, _> = methods.into_iter().map(|(name, v)| (name, Method::Call(v))).collect();
		self.methods.write().extend(methods);
	}

	/// Add supported notifications to this executor
	pub fn add_notifications(&self, notifications: HashMap<String, Box<NotificationCommand>>) {
		let notifications: HashMap<_, _> = notifications.into_iter().map(|(name, v)| (name, Method::Notify(v))).collect();
		self.methods.write().extend(notifications);
	}

	/// Add supported subscriptions to this executor
	pub fn add_subscriptions(&self, subscriptions: HashMap<(String, String, String), Box<SubscriptionCommand>>) {
		let mut methods = self.methods.write();

		for ((subscribe, subscription, unsubscribe), command) in subscriptions.into_iter() {
			let command = Arc::new(command);
			methods.insert(subscribe, Method::Subscribe(command.clone(), subscription));
			methods.insert(unsubscribe, Method::Unsubscribe(command));
		}
	}

	/// Execute method identified by `name` with given `params`.
	pub fn execute_method<A: 'static>(&self, name: String, params: Params, handler: Handler<A, Data, SubscriptionOutput>, session: Option<Session>) {
		match (self.methods.read().get(&name), session) {
			(Some(&Method::Call(ref command)), _) => {
				command.execute(params, handler.into());
			},
			(Some(&Method::Subscribe(ref subscribe, ref subscription_name)), Some(ref session)) => {
				subscribe.execute(Subscription::Open {
					params: params,
					subscriber: handler.into_subscriber(session.clone(), subscription_name.clone(), subscribe.clone()),
				});
			},
			(Some(&Method::Unsubscribe(ref unsubscribe)), Some(ref session)) => {
				if let Params::Array(params) = params {
					if let Some(id) = params.into_iter().next() {
						session.remove_subscription(name, id.clone());
						unsubscribe.execute(Subscription::Close {
							id: id,
							ready: handler.into(),
						});
						return;
					}
				}

				handler.send(Err(Error::new(ErrorCode::InvalidParams)))
			},
			(Some(&Method::Subscribe(_, _)), None) | (Some(&Method::Unsubscribe(_)), None) => {
				handler.send(Err(Error::new(ErrorCode::SessionNotSupported)))
			},
			_ => handler.send(Err(Error::new(ErrorCode::MethodNotFound))),
		};
	}

	/// Execute notification identified by `name` with given `params`.
	pub fn execute_notification(&self, name: String, params: Params) {
		if let Some(&Method::Notify(ref command)) = self.methods.read().get(&name) {
			command.execute(params)
		}
	}
}
