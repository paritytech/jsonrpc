//! jsonrpc io
use std::sync::Arc;
use std::collections::HashMap;
use super::{MethodCommand, NotificationCommand, Params, Value, Error, Response, Output, Version, Id, ErrorCode, RequestHandler, Request, Failure};
use serde_json;

struct DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Result<Value, Error>,
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F
}

impl<T, F> MethodCommand for DelegateMethod <T, F> where 
	F: Fn(&T, Params) -> Result<Value, Error>, 
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&mut self, params: Params) -> Result<Value, Error> {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

struct DelegateNotification<T, F> where
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F
}

impl<T, F> NotificationCommand for DelegateNotification<T, F> where 
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&mut self, params: Params) {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

pub struct IoDelegate<T> where T: Send + Sync + 'static {
	delegate: Arc<T>,
	methods: HashMap<String, Box<MethodCommand>>,
	notifications: HashMap<String, Box<NotificationCommand>>
}

impl<T> IoDelegate<T> where T: Send + Sync + 'static {
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			delegate: delegate,
			methods: HashMap::new(),
			notifications: HashMap::new()
		}
	}

	pub fn add_method<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params) -> Result<Value, Error> + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.methods.insert(name.to_owned(), Box::new(DelegateMethod {
			delegate: delegate,
			closure: closure
		}));
	}

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
/// 	let mut io = IoHandler::new();
/// 	struct SayHello;
/// 	impl MethodCommand for SayHello {
/// 		fn execute(&mut self, _params: Params) -> Result<Value, Error> {
/// 			Ok(Value::String("hello".to_string()))
/// 		}
/// 	}
///
/// 	io.add_method("say_hello", SayHello);
///
/// 	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
/// 	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;
///
/// 	assert_eq!(io.handle_request(request), Some(response.to_string()));
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
	pub fn new() -> Self {
		IoHandler {
			request_handler: RequestHandler::new()
		}
	}

	#[inline]
	pub fn add_method<C>(&mut self, name: &str, command: C) where C: MethodCommand + 'static {
		self.request_handler.add_method(name.to_owned(), Box::new(command))
	}

	#[inline]
	pub fn add_notification<C>(&mut self, name: &str, command: C) where C: NotificationCommand + 'static {
		self.request_handler.add_notification(name.to_owned(), Box::new(command))
	}

	pub fn add_delegate<D>(&mut self, delegate: IoDelegate<D>) where D: Send + Sync {
		self.request_handler.add_methods(delegate.methods);
		self.request_handler.add_notifications(delegate.notifications);
	}

	pub fn handle_request<'a>(&mut self, request_str: &'a str) -> Option<String> {
		match read_request(request_str) {
			Ok(request) => self.request_handler.handle_request(request).map(write_response),
			Err(error) => Some(write_response(Response::Single(Output::Failure(Failure {
				id: Id::Null,
				jsonrpc: Version::V2,
				error: error
			}))))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::super::*;

	#[test]
	fn test_io_handler() {
		let mut io = IoHandler::new();
		
		struct SayHello;
		impl MethodCommand for SayHello {
			fn execute(&mut self, _params: Params) -> Result<Value, Error> {
				Ok(Value::String("hello".to_string()))
			}
		}

		io.add_method("say_hello", SayHello);
		
		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request(request), Some(response.to_string()));
	}
}
