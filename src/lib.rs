//! jsonrpc http server.
//! 
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//! 
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//! 
//! struct SayHello;
//! impl MethodCommand for SayHello {
//! 	fn execute(&self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//! 
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let _server = Server::start(&"127.0.0.1:3030".parse().unwrap(), Arc::new(io), AccessControlAllowOrigin::Null);
//! }
//! ```

extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;

use std::sync::Arc;
use std::io::{Read, Write};
use std::net::SocketAddr;
use hyper::header::{Headers, Allow, ContentType, AccessControlAllowHeaders};
use hyper::method::Method;
use hyper::server::{Request, Response};
use hyper::{Next, Encoder, Decoder};
use hyper::net::HttpStream;
use unicase::UniCase;
use self::jsonrpc::{IoHandler};

pub use hyper::header::AccessControlAllowOrigin;

pub type ServerResult = Result<Server, RpcServerError>;

/// RPC Server startup error
#[derive(Debug)]
pub enum RpcServerError {
	IoError(std::io::Error),
	Other(hyper::error::Error),
}

impl From<hyper::error::Error> for RpcServerError {
	fn from(err: hyper::error::Error) -> Self {
		match err {
			hyper::error::Error::Io(e) => RpcServerError::IoError(e),
			e => RpcServerError::Other(e)
		}
	}
}
/// jsonrpc http request handler.
pub struct ServerHandler {
	jsonrpc_handler: Arc<IoHandler>,
	cors_domain: AccessControlAllowOrigin,
	request: String,
	response: Option<String>,
}

impl ServerHandler {
	/// Create new request handler.
	pub fn new(jsonrpc_handler: Arc<IoHandler>, cors_domain: AccessControlAllowOrigin) -> Self {
		ServerHandler {
			jsonrpc_handler: jsonrpc_handler,
			cors_domain: cors_domain,
			request: String::new(),
			response: None,
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

impl hyper::server::Handler<HttpStream> for ServerHandler {
    fn on_request(&mut self, request: Request) -> Next {
		match *request.method() {
			Method::Options => {
				self.response = Some(String::new());
				Next::write()
			},
			Method::Post => Next::read(),
			_ => Next::write(),
		}
	}

    /// This event occurs each time the `Request` is ready to be read from.
    fn on_request_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
		match decoder.read_to_string(&mut self.request) {
			Ok(0) => {
				self.response = self.jsonrpc_handler.handle_request(&self.request);
				Next::write()
			}
			Ok(_) => {
				Next::read()
			}
			Err(e) => match e.kind() {
				::std::io::ErrorKind::WouldBlock => Next::read(),
				_ => {
					//trace!("Read error: {}", e);
					Next::end()
				}
			}
		}
	}

    /// This event occurs after the first time this handled signals `Next::write()`.
    fn on_response(&mut self, response: &mut Response) -> Next {
		*response.headers_mut() = self.response_headers();
		if self.response.is_none() {
			response.set_status(hyper::status::StatusCode::MethodNotAllowed);
		}
		Next::write()
	}

    /// This event occurs each time the `Response` is ready to be written to.
    fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		if let Some(ref response) = self.response {
			encoder.write(response.as_bytes()).unwrap();
		}
		Next::end()
	}
}

/// jsonrpc http server.
///
/// ```no_run
/// extern crate jsonrpc_core;
/// extern crate jsonrpc_http_server;
///
/// use std::sync::Arc;
/// use jsonrpc_core::*;
/// use jsonrpc_http_server::*;
///
/// struct SayHello;
/// impl MethodCommand for SayHello {
/// 	fn execute(&self, _params: Params) -> Result<Value, Error> {
/// 		Ok(Value::String("hello".to_string()))
/// 	}
/// }
///
/// fn main() {
/// 	let io = IoHandler::new();
/// 	io.add_method("say_hello", SayHello);
/// 	let _server = Server::start(&"127.0.0.1:3030".parse().unwrap(), Arc::new(io), AccessControlAllowOrigin::Null);
/// }
/// ```
pub struct Server {
	server: Option<hyper::server::Listening>,
}

impl Server {
	pub fn start(addr: &SocketAddr, jsonrpc_handler: Arc<IoHandler>, cors_domain: AccessControlAllowOrigin) -> ServerResult {
		let srv = try!(try!(hyper::Server::http(addr)).handle(move |_| ServerHandler::new(jsonrpc_handler.clone(), cors_domain.clone())));
		Ok(Server {
			server: Some(srv),
		})
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}
