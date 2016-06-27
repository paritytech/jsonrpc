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
//! 	let _server = Server::start(&"127.0.0.1:3030".parse().unwrap(), Arc::new(io), vec![AccessControlAllowOrigin::Null]);
//! }
//! ```

extern crate hyper;
extern crate unicase;
extern crate jsonrpc_core as jsonrpc;

mod cors;

use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::io::{Read, Write};
use std::net::SocketAddr;
use hyper::header::{Headers, Allow, ContentType, AccessControlAllowHeaders};
use hyper::method::Method;
use hyper::mime;
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

/// PanicHandling function
pub struct PanicHandler {
	pub handler: Arc<Mutex<Option<Box<Fn() -> () + Send + 'static>>>>
}

/// jsonrpc http request handler.
pub struct ServerHandler {
	panic_handler: PanicHandler,
	jsonrpc_handler: Arc<IoHandler>,
	cors_domains: Vec<AccessControlAllowOrigin>,
	request: String,
	origin: Option<String>,
	response: Option<String>,
	error_code:  hyper::status::StatusCode,
	write_pos: usize,
}

impl Drop for ServerHandler {
	fn drop(&mut self) {
		if ::std::thread::panicking() {
			let handler = self.panic_handler.handler.lock().unwrap();
			if let Some(ref h) = *handler.deref() {
				h();
			}
		}
	}
}


impl ServerHandler {
	/// Create new request handler.
	pub fn new(jsonrpc_handler: Arc<IoHandler>, cors_domains: Vec<AccessControlAllowOrigin>, panic_handler: PanicHandler) -> Self {
		ServerHandler {
			panic_handler: panic_handler,
			jsonrpc_handler: jsonrpc_handler,
			cors_domains: cors_domains,
			request: String::new(),
			origin: None,
			response: None,
			error_code: hyper::status::StatusCode::MethodNotAllowed,
			write_pos: 0,
		}
	}

	fn response_headers(&self, origin: &Option<String>) -> Headers {
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

		if let Some(cors_domain) = cors::get_cors_header(&self.cors_domains, origin) {
			headers.set(cors_domain);
		}

		headers
	}
}

impl hyper::server::Handler<HttpStream> for ServerHandler {
	fn on_request(&mut self, request: Request<HttpStream>) -> Next {
		// Read origin
		self.origin = cors::read_origin(&request);

		match *request.method() {
			// Don't validate content type on options
			Method::Options => {
				self.response = Some(String::new());
				Next::write()
			},
			Method::Post => {
				// Validate the ContentType header
				// to prevent Cross-Origin XHRs with text/plain
				let content_type = request.headers().get::<ContentType>();
				if let Some(&ContentType(mime::Mime(mime::TopLevel::Application, mime::SubLevel::Json, _))) = content_type {
					Next::read()
				} else {
					self.error_code = hyper::status::StatusCode::UnsupportedMediaType;
					// Just return error
					Next::write()
				}
			},
			_ => Next::write(),
		}
	}

	/// This event occurs each time the `Request` is ready to be read from.
	fn on_request_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
		match decoder.read_to_string(&mut self.request) {
			Ok(0) => {
				self.response = self.jsonrpc_handler.handle_request(&self.request);
				match self.response {
					Some(ref mut r) => r.push('\n'),
					_ => {
						self.error_code = hyper::status::StatusCode::BadRequest;
					}
				}
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
		*response.headers_mut() = self.response_headers(&self.origin);
		if self.response.is_none() {
			response.set_status(self.error_code);
		}
		Next::write()
	}

	/// This event occurs each time the `Response` is ready to be written to.
	fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		if self.error_code == hyper::status::StatusCode::UnsupportedMediaType {
			self.response = Some("Content-Type: application/json required\n".into());
		}
		if let Some(ref response) = self.response {
			let bytes = response.as_bytes();
            if bytes.len() == self.write_pos {
				Next::end()
			} else {
				println!("Writing {}..{}", self.write_pos, bytes.len());
				match encoder.write(&bytes[self.write_pos..]) {
					Ok(0) => {
						Next::write()
					}
					Ok(bytes) => {
						self.write_pos += bytes;
						Next::write()
					}
					Err(e) => match e.kind() {
						::std::io::ErrorKind::WouldBlock => Next::write(),
						_ => {
							//trace!("Write error: {}", e);
							Next::end()
						}
					}
				}
			}
		} else {
			Next::end()
		}
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
/// 	let _server = Server::start(&"127.0.0.1:3030".parse().unwrap(), Arc::new(io), vec![AccessControlAllowOrigin::Null]);
/// }
/// ```
pub struct Server {
	server: Option<hyper::server::Listening>,
	panic_handler: Arc<Mutex<Option<Box<Fn() -> () + Send>>>>
}

impl Server {
	pub fn start(addr: &SocketAddr, jsonrpc_handler: Arc<IoHandler>, cors_domains: Vec<AccessControlAllowOrigin>) -> ServerResult {
		let panic_handler = Arc::new(Mutex::new(None));
		let panic_for_server = panic_handler.clone();
		let (l, srv) = try!(try!(hyper::Server::http(addr)).handle(move |_| {
			let handler = PanicHandler { handler: panic_for_server.clone() };
			ServerHandler::new(jsonrpc_handler.clone(), cors_domains.clone(), handler)
		}));
		::std::thread::spawn(move || {
			srv.run();
 		});
		Ok(Server {
			server: Some(l),
			panic_handler: panic_handler,
		})
	}

	pub fn set_panic_handler<F>(&self, handler: F)
		where F : Fn() -> () + Send + 'static {
		*self.panic_handler.lock().unwrap() = Some(Box::new(handler));
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.server.take().unwrap().close()
	}
}
