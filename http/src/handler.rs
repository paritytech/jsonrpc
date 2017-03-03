use Rpc;

use std::sync::{mpsc, Arc};
use std::io::{self, Read};

use hyper::{self, mime, server, Next, Encoder, Decoder, Control};
use hyper::header::{Headers, Allow, ContentType, AccessControlAllowHeaders, AccessControlAllowOrigin};
use hyper::method::Method;
use hyper::net::HttpStream;
use parking_lot::Mutex;
use unicase::UniCase;

use jsonrpc::{Metadata, Middleware, NoopMiddleware};
use jsonrpc::futures::Future;
use jsonrpc_server_utils::{cors, hosts};
use request_response::{Request, Response};
use hosts_validator::is_host_header_valid;

/// Reads Origin header from the request.
pub fn read_origin<'a>(req: &'a hyper::server::Request<'a, HttpStream>) -> Option<&'a str> {
	match req.headers().get_raw("origin") {
		Some(ref v) if v.len() == 1 => {
			::std::str::from_utf8(&v[0]).ok()
		},
		_ => None
	}
}

/// Returns correct CORS header (if any) given list of allowed origins and current origin.
pub fn get_cors_header(origin: Option<&str>, allowed: &Option<Vec<cors::AccessControlAllowOrigin>>) -> Option<AccessControlAllowOrigin> {
	cors::get_cors_header(origin, allowed).map(|origin| {
		use self::cors::AccessControlAllowOrigin::*;
		match origin {
			Value(val) => AccessControlAllowOrigin::Value(val),
			Null => AccessControlAllowOrigin::Null,
			Any => AccessControlAllowOrigin::Any,
		}
	})
}

/// PanicHandling function
pub struct PanicHandler {
	/// Actual handler
	pub handler: Arc<Mutex<Option<Box<Fn() -> () + Send + 'static>>>>
}

/// jsonrpc http request handler.
pub struct ServerHandler<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	panic_handler: PanicHandler,
	jsonrpc_handler: Rpc<M, S>,
	cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<hosts::Host>>,
	metadata: Option<M>,
	control: Control,
	request: Request,
	response: Response,
	/// Asynchronous response waiting to be moved into `response` field.
	waiting_sender: mpsc::Sender<Response>,
	waiting_response: mpsc::Receiver<Response>,
}

impl<M: Metadata, S: Middleware<M>> Drop for ServerHandler<M, S> {
	fn drop(&mut self) {
		if ::std::thread::panicking() {
			let handler = self.panic_handler.handler.lock();
			if let Some(ref h) = *handler {
				h();
			}
		}
	}
}

impl<M: Metadata, S: Middleware<M>> ServerHandler<M, S> {
	/// Create new request handler.
	pub fn new(
		jsonrpc_handler: Rpc<M, S>,
		cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
		allowed_hosts: Option<Vec<hosts::Host>>,
		panic_handler: PanicHandler,
		control: Control,
	) -> Self {
		let (sender, receiver) = mpsc::channel();

		ServerHandler {
			panic_handler: panic_handler,
			jsonrpc_handler: jsonrpc_handler,
			cors_domains: cors_domains,
			allowed_hosts: allowed_hosts,
			control: control,
			metadata: None,
			request: Request::empty(),
			response: Response::method_not_allowed(),
			waiting_sender: sender,
			waiting_response: receiver,
		}
	}

	fn response_headers(&self, origin: &Option<String>) -> Headers {
		let mut headers = Headers::new();
		headers.set(self.response.content_type.clone());

		if let Some(cors_domain) = get_cors_header(origin.as_ref().map(|s| s.as_str()), &self.cors_domains) {
			headers.set(Allow(vec![
				Method::Options,
				Method::Post,
			]));
			headers.set(AccessControlAllowHeaders(vec![
				UniCase("origin".to_owned()),
				UniCase("content-type".to_owned()),
				UniCase("accept".to_owned()),
			]));
			headers.set(cors_domain);
		}

		headers
	}

	fn is_json(&self, content_type: Option<&ContentType>) -> bool {
		if let Some(&ContentType(mime::Mime(mime::TopLevel::Application, mime::SubLevel::Json, _))) = content_type {
			true
		} else {
			false
		}
	}
}

impl<M: Metadata, S: Middleware<M>> server::Handler<HttpStream> for ServerHandler<M, S> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		// Validate host
		if let Some(ref allowed_hosts) = self.allowed_hosts {
			if !is_host_header_valid(&request, allowed_hosts) {
				self.response = Response::host_not_allowed();
				return Next::write();
			}
		}

		// Read origin
		self.request.origin = read_origin(&request).map(String::from);
		self.metadata = Some(self.jsonrpc_handler.extractor.read_metadata(&request));

		match *request.method() {
			// Don't validate content type on options
			Method::Options => {
				self.response = Response::empty();
				Next::write()
			},
			// Validate the ContentType header
			// to prevent Cross-Origin XHRs with text/plain
			Method::Post if self.is_json(request.headers().get::<ContentType>()) => {
				Next::read()
			},
			Method::Post => {
				// Just return error
				self.response = Response::unsupported_content_type();
				Next::write()
			},
			_ => {
				self.response = Response::method_not_allowed();
				Next::write()
			}
		}
	}

	/// This event occurs each time the `Request` is ready to be read from.
	fn on_request_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
		match decoder.read_to_string(&mut self.request.content) {
			Ok(0) => {
				let metadata = self.metadata.take().unwrap_or_default();
				let future = self.jsonrpc_handler.handler.handle_request(&self.request.content, metadata);

				let sender = self.waiting_sender.clone();
				let control = self.control.clone();

				self.jsonrpc_handler.remote.spawn(move |_| {
						future.map(move |response| {
							let response = match response {
								None => Response::ok(String::new()),
								// Add new line to have nice output when using CLI clients (curl)
								Some(result) => Response::ok(format!("{}\n", result)),
							};

							let result = sender.send(response)
								.map_err(|e| format!("{:?}", e))
								.and_then(|_| {
									control.ready(Next::write()).map_err(|e| format!("{:?}", e))
								});

							if let Err(e) = result {
								warn!("Error while resuming async call: {:?}", e);
							}
						})
				});

				Next::wait()
			}
			Ok(_) => {
				Next::read()
			}
			Err(e) => match e.kind() {
				io::ErrorKind::WouldBlock => Next::read(),
				_ => {
					Next::end()
				}
			}
		}
	}

	/// This event occurs after the first time this handled signals `Next::write()`.
	fn on_response(&mut self, response: &mut server::Response) -> Next {
		// Check if there is a response pending
		if let Ok(output) = self.waiting_response.try_recv() {
			self.response = output;
		}

		*response.headers_mut() = self.response_headers(&self.request.origin);
		response.set_status(self.response.code);
		Next::write()
	}

	/// This event occurs each time the `Response` is ready to be written to.
	fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		let bytes = self.response.content.as_bytes();
		if bytes.len() == self.response.write_pos {
			return Next::end();
		}

		match encoder.write(&bytes[self.response.write_pos..]) {
			Ok(0) => {
				Next::write()
			}
			Ok(bytes) => {
				self.response.write_pos += bytes;
				Next::write()
			}
			Err(e) => match e.kind() {
				io::ErrorKind::WouldBlock => Next::write(),
				_ => {
					//trace!("Write error: {}", e);
					Next::end()
				}
			}
		}
	}
}

