use Rpc;

use std::sync::{mpsc, Arc};
use std::io::{self, Read};

use hyper::{self, mime, server, Next, Encoder, Decoder, Control};
use hyper::header::{self, Headers};
use hyper::method::Method;
use hyper::net::HttpStream;
use parking_lot::Mutex;
use unicase::UniCase;

use jsonrpc::{Metadata, Middleware, NoopMiddleware};
use jsonrpc::futures::Future;
use jsonrpc_server_utils::{cors, hosts};
use request_response::{Request, Response};

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

	fn response_headers(&self, cors_header: Option<header::AccessControlAllowOrigin>) -> Headers {
		let mut headers = Headers::new();
		headers.set(self.response.content_type.clone());

		if let Some(cors_domain) = cors_header {
			headers.set(header::Allow(vec![
				Method::Options,
				Method::Post,
			]));
			headers.set(header::AccessControlAllowHeaders(vec![
				UniCase("origin".to_owned()),
				UniCase("content-type".to_owned()),
				UniCase("accept".to_owned()),
			]));
			headers.set(cors_domain);
		}

		headers
	}

	fn get_header<'a>(req: &'a hyper::server::Request<'a, HttpStream>, header: &str) -> Option<&'a str> {
		match req.headers().get_raw(header) {
			Some(ref v) if v.len() == 1 => {
				::std::str::from_utf8(&v[0]).ok()
			},
			_ => None
		}
	}

	fn cors_header(
		origin: Option<&str>,
		allowed: &Option<Vec<cors::AccessControlAllowOrigin>>,
	) -> Option<header::AccessControlAllowOrigin> {
		cors::get_cors_header(origin, allowed).map(|origin| {
			use self::cors::AccessControlAllowOrigin::*;
			match origin {
				Value(val) => header::AccessControlAllowOrigin::Value(val),
				Null => header::AccessControlAllowOrigin::Null,
				Any => header::AccessControlAllowOrigin::Any,
			}
		})
	}

	fn is_json(content_type: Option<&header::ContentType>) -> bool {
		if let Some(&header::ContentType(
			mime::Mime(mime::TopLevel::Application, mime::SubLevel::Json, _)
		)) = content_type {
			true
		} else {
			false
		}
	}
}

impl<M: Metadata, S: Middleware<M>> server::Handler<HttpStream> for ServerHandler<M, S> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		// Validate host
		if !hosts::is_host_valid(Self::get_header(&request, "host"), &self.allowed_hosts) {
			self.response = Response::host_not_allowed();
			return Next::write();
		}

		// Read origin
		self.request.cors_header = Self::cors_header(Self::get_header(&request, "origin"), &self.cors_domains);
		// Read metadata
		self.metadata = Some(self.jsonrpc_handler.extractor.read_metadata(&request));

		match *request.method() {
			// Validate the ContentType header
			// to prevent Cross-Origin XHRs with text/plain
			Method::Post if Self::is_json(request.headers().get::<header::ContentType>()) => {
				Next::read()
			},
			// Just return error for unsupported content type
			Method::Post => {
				self.response = Response::unsupported_content_type();
				Next::write()
			},
			// Don't validate content type on options
			Method::Options => {
				self.response = Response::empty();
				Next::write()
			},
			// Disallow other methods.
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

		let cors_header = self.request.cors_header.take();
		*response.headers_mut() = self.response_headers(cors_header);
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

