use cors;

use std::sync::{mpsc, Arc, Mutex};
use std::io::{self, Read};
use unicase::UniCase;
use hyper::{mime, server, Next, Encoder, Decoder, Control};
use hyper::header::{Headers, Allow, ContentType, AccessControlAllowHeaders};
use hyper::method::Method;
use hyper::net::HttpStream;
use hyper::header::AccessControlAllowOrigin;
use jsonrpc::{IoHandler, GenericIoHandler, ResponseHandler};
use request_response::{Request, Response};
use hosts_validator::is_host_header_valid;

/// PanicHandling function
pub struct PanicHandler {
	/// Actual handler
	pub handler: Arc<Mutex<Option<Box<Fn() -> () + Send + 'static>>>>
}

/// RPC Requests handler with additional metadata support.
pub trait RpcHandler: Send + Sync {
	/// Type of metadata
	type Metadata: Send;

	/// Read the metadata from the request
	fn read_metadata(&self, &server::Request<HttpStream>) -> Option<Self::Metadata> {
		None
	}

	/// Handle request with additional metadata.
	fn handle_request<H>(&self, request_str: &str, response_handler: H, meta: Option<Self::Metadata>) where
		H: ResponseHandler<Option<String>, Option<String>> + 'static;
}

impl RpcHandler for IoHandler {
	type Metadata = ();

	fn handle_request<H>(&self, request_str: &str, response_handler: H, _meta: Option<Self::Metadata>) where
		H: ResponseHandler<Option<String>, Option<String>> + 'static
	{
		GenericIoHandler::handle_request(self, request_str, response_handler)
	}
}

/// jsonrpc http request handler.
pub struct ServerHandler<T: RpcHandler> {
	panic_handler: PanicHandler,
	jsonrpc_handler: Arc<T>,
	cors_domains: Option<Vec<AccessControlAllowOrigin>>,
	allowed_hosts: Option<Vec<String>>,
	metadata: Option<T::Metadata>,
	control: Control,
	request: Request,
	response: Response,
	/// Asynchronous response waiting to be moved into `response` field.
	waiting_sender: mpsc::Sender<Response>,
	waiting_response: mpsc::Receiver<Response>,
}

impl<T: RpcHandler> Drop for ServerHandler<T> {
	fn drop(&mut self) {
		if ::std::thread::panicking() {
			let handler = self.panic_handler.handler.lock().unwrap();
			if let Some(ref h) = *handler {
				h();
			}
		}
	}
}

impl<T: RpcHandler> ServerHandler<T> {
	/// Create new request handler.
	pub fn new(
		jsonrpc_handler: Arc<T>,
		cors_domains: Option<Vec<AccessControlAllowOrigin>>,
		allowed_hosts: Option<Vec<String>>,
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
		headers.set(Allow(vec![
			Method::Options,
			Method::Post,
		]));
		headers.set(AccessControlAllowHeaders(vec![
			UniCase("origin".to_owned()),
			UniCase("content-type".to_owned()),
			UniCase("accept".to_owned()),
		]));

		if let Some(cors_domain) = cors::get_cors_header(&self.cors_domains, origin) {
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

impl<T: RpcHandler> server::Handler<HttpStream> for ServerHandler<T> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		// Validate host
		if let Some(ref allowed_hosts) = self.allowed_hosts {
			if !is_host_header_valid(&request, allowed_hosts) {
				self.response = Response::host_not_allowed();
				return Next::write();
			}
		}

		// Read origin
		self.request.origin = cors::read_origin(&request);
		self.metadata = self.jsonrpc_handler.read_metadata(&request);

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
				let control = self.control.clone();
				let sender = self.waiting_sender.clone();

				let metadata = self.metadata.take();

				self.jsonrpc_handler.handle_request(&self.request.content, move |response| {
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
				}, metadata);

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

