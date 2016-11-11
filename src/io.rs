//! jsonrpc io
use std::sync::Arc;
use std::sync::mpsc;
use std::collections::HashMap;
use serde_json;

use control::{Ready, ResponseHandler, Handler, Data, Subscription, Session};
use commander::{MethodCommand, SyncMethod, SyncMethodCommand, NotificationCommand, SubscriptionCommand};
use request_handler::RequestHandler;
use params::Params;
use id::Id;
use version::Version;
use request::Request;
use response::{Response, Failure};
use error::{Error, ErrorCode};
use super::Value;

struct DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Data,
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F,
}

impl<T, F> MethodCommand for DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Data,
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params, ready: Ready) {
		let closure = &self.closure;
		ready.ready(closure(&self.delegate, params))
	}
}

struct DelegateAsyncMethod<T, F> where
	F: Fn(&T, Params, Ready),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F,
}

impl<T, F> MethodCommand for DelegateAsyncMethod<T, F> where
	F: Fn(&T, Params, Ready),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params, ready: Ready) {
		let closure = &self.closure;
		closure(&self.delegate, params, ready)
	}
}

struct DelegateNotification<T, F> where
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F,
}

impl<T, F> NotificationCommand for DelegateNotification<T, F> where
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params) {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

struct DelegateSubscription<T, F> where
	F: Fn(&T, Subscription),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F,
}

impl<T, F> SubscriptionCommand for DelegateSubscription<T, F> where
	F: Fn(&T, Subscription),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, subscription: Subscription) {
		let closure = &self.closure;
		closure(&self.delegate, subscription)
	}
}

/// A set of RPC methods and notifications tied to single `delegate` struct.
pub struct IoDelegate<T> where T: Send + Sync + 'static {
	delegate: Arc<T>,
	methods: HashMap<String, Box<MethodCommand>>,
	notifications: HashMap<String, Box<NotificationCommand>>,
	subscriptions: HashMap<(String, String), Box<SubscriptionCommand>>,
}

impl<T> IoDelegate<T> where T: Send + Sync + 'static {
	/// Creates new `IoDelegate`
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			delegate: delegate,
			methods: HashMap::new(),
			notifications: HashMap::new(),
			subscriptions: HashMap::new(),
		}
	}

	/// Add new supported method
	pub fn add_method<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params) -> Result<Value, Error> + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.methods.insert(name.to_owned(), Box::new(DelegateMethod {
			delegate: delegate,
			closure: closure
		}));
	}

	/// Add new supported asynchronous method
	pub fn add_async_method<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params, Ready) + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.methods.insert(name.to_owned(), Box::new(DelegateAsyncMethod {
			delegate: delegate,
			closure: closure
		}));
	}

	/// Add new supported subscription
	pub fn add_subscription<F>(&mut self, subscribe: &str, unsubscribe: &str, closure: F) where F: Fn(&T, Subscription) + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.subscriptions.insert((subscribe.into(), unsubscribe.into()), Box::new(DelegateSubscription {
			delegate: delegate,
			closure: closure,
		}));
	}

	/// Add new supported notification
	pub fn add_notification<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params) + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.notifications.insert(name.to_owned(), Box::new(DelegateNotification {
			delegate: delegate,
			closure: closure
		}));
	}
}

/// Should be used to handle jsonrpc io.
///
/// ```rust
/// extern crate jsonrpc_core;
/// use jsonrpc_core::*;
///
/// fn main() {
/// 	let io = IoHandler::new();
/// 	struct SayHello;
///
/// 	// Implement `SyncMethodCommand` or `AsyncMethodCommand`
/// 	impl SyncMethodCommand for SayHello {
/// 		fn execute(&self, _params: Params) -> Result<Value, Error>  {
/// 			Ok(Value::String("hello".to_string()))
/// 		}
/// 	}
///
/// 	io.add_method("say_hello", SayHello);
/// 	// Or just use closures
/// 	io.add_async_method("say_hello_async", |_params: Params, ready: Ready| {
///			ready.ready(Ok(Value::String("hello".to_string())))
/// 	});
///
/// 	let request1 = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
/// 	let request2 = r#"{"jsonrpc": "2.0", "method": "say_hello_async", "params": [42, 23], "id": 1}"#;
/// 	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;
///
/// 	assert_eq!(io.handle_request_sync(request1), Some(response.to_string()));
/// 	assert_eq!(io.handle_request_sync(request2), Some(response.to_string()));
/// }
/// ```
pub struct IoHandler {
	request_handler: RequestHandler
}

fn read_request(request_str: &str) -> Result<Request, Error> {
	serde_json::from_str(request_str).map_err(|_| Error::new(ErrorCode::ParseError))
}

fn write_response(response: Response) -> String {
	// this should never fail
	serde_json::to_string(&response).unwrap()
}

impl IoHandler {
	/// Creates new `IoHandler`
	pub fn new() -> Self {
		IoHandler {
			request_handler: RequestHandler::new()
		}
	}

	/// Add supported method
	pub fn add_method<C>(&self, name: &str, command: C) where C: SyncMethodCommand + 'static {
		self.request_handler.add_method(name.into(), SyncMethod {
			command: command
		})
	}

	/// Add supported asynchronous method
	pub fn add_async_method<C>(&self, name: &str, command: C) where C: MethodCommand + 'static {
		self.request_handler.add_method(name.into(), command)
	}

	/// Add supported notification
	pub fn add_notification<C>(&self, name: &str, command: C) where C: NotificationCommand + 'static {
		self.request_handler.add_notification(name.into(), command)
	}

	/// Add supported subscription
	pub fn add_subscription<C>(&self, subscribe: &str, unsubscribe: &str, command: C) where C: SubscriptionCommand + 'static {
		self.request_handler.add_subscription(subscribe.into(), unsubscribe.into(), command);
	}

	/// Add delegate with supported methods.
	pub fn add_delegate<D>(&self, delegate: IoDelegate<D>) where D: Send + Sync {
		self.request_handler.add_methods(delegate.methods);
		self.request_handler.add_notifications(delegate.notifications);
		self.request_handler.add_subscriptions(delegate.subscriptions);
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync(&self, request_str: &str) -> Option<String> {
		let (tx, rx) = mpsc::channel();
		self.handle_request(request_str, move |response: Option<String>| {
			tx.send(response).expect("Receiver is never dropped.");
		});
		// Wait for response.
		rx.recv().expect("Transmitting end is never dropped.")
	}

	/// Handle given request asynchronously.
	pub fn handle_request<H: ResponseHandler<Option<String>> + 'static>(&self, request_str: &str, response_handler: H) {
		self.handle(request_str, response_handler, None);
	}

	fn handle<H: ResponseHandler<Option<String>> + 'static>(&self, request_str: &str, response_handler: H, session: Option<Session>) {
		trace!(target: "rpc", "Request: {} in session.", request_str);

		let handler = Handler::new(response_handler, move |response: Option<Response>| {
			let response = response.map(write_response);
			debug!(target: "rpc", "Response: {:?}", response);
			response
		});

		let request = read_request(request_str);
		match request {
			Ok(request) => self.request_handler.handle_request(request, handler, session),
			Err(error) => handler.send(Some(Response::from(Failure {
				id: Id::Null,
				jsonrpc: Version::V2,
				error: error
			}))),
		}
	}
}

/// Io Handler with session support
pub trait IoSessionHandler {
	/// Returns a new session object.
	fn session(&self) -> IoSession;
}

impl IoSessionHandler for Arc<IoHandler> {
	fn session(&self) -> IoSession {
		IoSession::new(self.clone())
	}
}

/// Represents a single client connected to this RPC server.
/// The client may send many requests.
pub struct IoSession {
	io_handler: Arc<IoHandler>,
	session: Session,
}

impl IoSession {
	/// Opens up a new session to handle many request and subscriptions.
	/// It should represent a single client.
	pub fn new(handler: Arc<IoHandler>) -> IoSession {
		IoSession {
			io_handler: handler,
			session: Session::default(),
		}
	}

	/// Handle a request within this session.
	pub fn handle_request<H: ResponseHandler<Option<String>> + 'static>(&self, request_str: &str, handler: H) {
		self.io_handler.handle(request_str, handler, Some(self.session.clone()));
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{mpsc, Arc};
	use std::time::Duration;
	use std::thread;
	use super::super::*;

	#[test]
	fn test_io_handler() {
		let io = IoHandler::new();

		struct SayHello;
		impl SyncMethodCommand for SayHello {
			fn execute(&self, _params: Params) -> Result<Value, Error> {
				Ok(Value::String("hello".to_string()))
			}
		}

		io.add_method("say_hello", SayHello);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let io = IoHandler::new();

		struct SayHelloAsync;
		impl MethodCommand for SayHelloAsync {
			fn execute(&self, _params: Params, ready: Ready) {
				ready.ready(Ok(Value::String("hello".to_string())))
			}
		}

		io.add_async_method("say_hello", SayHelloAsync);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler_thread() {
		let io = IoHandler::new();

		struct SayHelloAsync;
		impl MethodCommand for SayHelloAsync {
			fn execute(&self, _params: Params, ready: Ready) {
				thread::spawn(|| {
					thread::sleep(Duration::from_secs(1));
					ready.ready(Ok(Value::String("hello".to_string())))
				});
			}
		}

		io.add_async_method("say_hello", SayHelloAsync);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_session_handler_with_subscription() {
		let io = Arc::new(IoHandler::new());

		struct SayHelloSubscription;
		impl SubscriptionCommand for SayHelloSubscription {
			fn execute(&self, subscription: Subscription) {
				match subscription {
					Subscription::Open { subscriber, .. } => {
						let subscriber = subscriber.assign_id(Value::U64(1));
						thread::spawn(move || {
							thread::sleep(Duration::from_millis(10));
							subscriber.send(Ok(Value::String("hello".to_string())));
							thread::sleep(Duration::from_millis(10));
							subscriber.send(Ok(Value::String("world".to_string())));
						});
					},
					Subscription::Close { id, .. } => {
						assert_eq!(id, Value::U64(1));
					},
				}
			}
		}

		io.add_subscription("say_hello", "stop_saying_hello", SayHelloSubscription);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let id = r#"{"jsonrpc":"2.0","result":1,"id":1}"#.to_owned();
		let response1 = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#.to_owned();
		let response2 = r#"{"jsonrpc":"2.0","result":"world","id":1}"#.to_owned();

		let (tx, rx) = mpsc::channel();
		let session = io.session();
		session.handle_request(request, move |res| tx.send(res).unwrap());

		assert_eq!(rx.recv().unwrap(), Some(id));
		assert_eq!(rx.recv().unwrap(), Some(response1));
		assert_eq!(rx.recv().unwrap(), Some(response2));
		drop(session);
		assert!(rx.recv().is_err());
	}

}
