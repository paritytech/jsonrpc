//! jsonrpc http server.
//! 
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//! 
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//! 
//! struct SayHello;
//! impl MethodCommand for SayHello {
//! 	fn execute(&mut self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//! 
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let server = Server::new(io, 1);
//! 	server.start("127.0.0.1:3030", AccessControlAllowOrigin::Null);
//! }
//! ```

extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;

use std::thread;
use std::sync::Mutex;
use std::io::Read;
use hyper::header::{Headers, Allow, ContentType, AccessControlAllowHeaders};
use hyper::method::Method;
use unicase::UniCase;
use self::jsonrpc::{IoHandler};

pub use hyper::header::AccessControlAllowOrigin;

/// jsonrpc http request handler.
struct ServerHandler {
	jsonrpc_handler: Mutex<IoHandler>,
	cors_domain: AccessControlAllowOrigin,
}

impl ServerHandler {
	/// Create new request handler.
	fn new(jsonrpc_handler: IoHandler, cors_domain: AccessControlAllowOrigin) -> Self {
		ServerHandler {
			jsonrpc_handler: Mutex::new(jsonrpc_handler),
			cors_domain: cors_domain
		}
	}

	fn response_headers(&self) -> Headers {
		let mut headers = Headers::new();
		headers.set(
			Allow(vec![
				Method::Options, Method::Post
			])
		);
		headers.set(ContentType::json());
		headers.set(
			AccessControlAllowHeaders(vec![
				UniCase("origin".to_owned()),
				UniCase("content-type".to_owned()),
				UniCase("accept".to_owned()),
			])
		);
		headers.set(self.cors_domain.clone());
		headers
	}
}

impl hyper::server::Handler for ServerHandler {
	fn handle(&self, mut req: hyper::server::Request, mut res: hyper::server::Response) {
		match req.method {
			Method::Options => {
				*res.headers_mut() = self.response_headers();
			},
			Method::Post => { 
				let mut body = String::new();
				if let Err(_) = req.read_to_string(&mut body) {
					// TODO: return proper jsonrpc error instead
					*res.status_mut() = hyper::status::StatusCode::MethodNotAllowed;
					return;
				}
				if let Some(response) = self.jsonrpc_handler.lock().unwrap().handle_request(&body) {
					*res.headers_mut() = self.response_headers();
					res.send(response.as_ref()).unwrap();
				}
			},
			_ => *res.status_mut() = hyper::status::StatusCode::MethodNotAllowed
		}
	}
}

/// jsonrpc http server.
/// 
/// ```no_run
/// extern crate jsonrpc_core;
/// extern crate jsonrpc_http_server;
/// 
/// use jsonrpc_core::*;
/// use jsonrpc_http_server::*;
/// 
/// struct SayHello;
/// impl MethodCommand for SayHello {
/// 	fn execute(&mut self, _params: Params) -> Result<Value, Error> {
/// 		Ok(Value::String("hello".to_string()))
/// 	}
/// }
/// 
/// fn main() {
/// 	let mut io = IoHandler::new();
/// 	io.add_method("say_hello", SayHello);
/// 	let server = Server::new(io, 1);
/// 	server.start("127.0.0.1:3030", AccessControlAllowOrigin::Null);
/// }
/// ```
pub struct Server {
	jsonrpc_handler: IoHandler,
	threads: usize
}

impl Server {
	pub fn new(jsonrpc_handler: IoHandler, threads: usize) -> Self { 
		Server {
			jsonrpc_handler: jsonrpc_handler,
			threads: threads
		}
	}

	pub fn start(self, addr: &str, cors_domain: AccessControlAllowOrigin) {
		hyper::Server::http(addr).unwrap().handle_threads(ServerHandler::new(self.jsonrpc_handler, cors_domain), self.threads).unwrap();
	}

	pub fn start_async(self, addr: &str, cors_domain: AccessControlAllowOrigin) {
		let address = addr.to_owned();
		thread::Builder::new().name("jsonrpc_http".to_string()).spawn(move || {
			self.start(address.as_ref(), cors_domain);
		}).unwrap();
	}
}

