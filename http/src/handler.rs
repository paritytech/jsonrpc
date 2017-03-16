use Rpc;

use std::sync::{mpsc, Arc};
use std::io::{self, Read};
use std::ops::{Deref, DerefMut};

use hyper::{mime, server, Next, Encoder, Decoder, Control};
use hyper::header::{self, Headers};
use hyper::method::Method;
use hyper::net::HttpStream;
use unicase::UniCase;

use jsonrpc::{Metadata, Middleware, NoopMiddleware};
use jsonrpc::futures::Future;
use request_response::{Request, Response};
use jsonrpc_server_utils::{cors, hosts};

use {utils, RequestMiddleware, RequestMiddlewareAction};

/// jsonrpc http request handler.
pub struct ServerHandler<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	allowed_hosts: Option<Vec<hosts::Host>>,
	middleware: Arc<RequestMiddleware>,
	handler: Handler<M, S>,
}

impl<M: Metadata, S: Middleware<M>> ServerHandler<M, S> {
	/// Create new request handler.
	pub fn new(
		jsonrpc_handler: Rpc<M, S>,
		cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
		allowed_hosts: Option<Vec<hosts::Host>>,
		middleware: Arc<RequestMiddleware>,
		control: Control,
	) -> Self {
		let (sender, receiver) = mpsc::channel();

		ServerHandler {
			allowed_hosts: allowed_hosts,
			middleware: middleware,
			handler: Handler::Rpc(RpcHandler {
				cors_domains: cors_domains,
				jsonrpc_handler: jsonrpc_handler,
				control: control,
				metadata: None,
				request: Request::empty(),
				response: Response::method_not_allowed(),
				waiting_sender: sender,
				waiting_response: receiver,
			})
		}
	}
}

impl<M: Metadata, S: Middleware<M>> server::Handler<HttpStream> for ServerHandler<M, S> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		let action = self.middleware.on_request(&request);

		let (should_validate_hosts, handler) = match action {
			RequestMiddlewareAction::Proceed => (true, None),
			RequestMiddlewareAction::Respond { should_validate_hosts, handler } => (should_validate_hosts, Some(handler)),
		};

		// Validate host
		if should_validate_hosts && !utils::is_host_allowed(&request, &self.allowed_hosts) {
			self.handler = Handler::Error(Response::host_not_allowed());
			return Next::write();
		}

		// Replace handler with the one returned by middleware.
		if let Some(handler) = handler {
			self.handler = Handler::Middleware(handler);
		}

		// Fire handler
		self.handler.on_request(request)
	}

	/// This event occurs each time the `Request` is ready to be read from.
	fn on_request_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
		self.handler.on_request_readable(decoder)
	}

	/// This event occurs after the first time this handled signals `Next::write()`.
	fn on_response(&mut self, response: &mut server::Response) -> Next {
		self.handler.on_response(response)
	}

	/// This event occurs each time the `Response` is ready to be written to.
	fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		self.handler.on_response_writable(encoder)
	}
}

enum Handler<M: Metadata, S: Middleware<M>> {
	Rpc(RpcHandler<M, S>),
	Error(Response),
	Middleware(Box<server::Handler<HttpStream> + Send>),
}

impl<M: Metadata, S: Middleware<M>> Deref for Handler<M, S> {
	type Target = server::Handler<HttpStream>;

	fn deref(&self) -> &Self::Target {
		match *self {
			Handler::Rpc(ref handler) => handler,
			Handler::Error(ref handler) => handler,
			Handler::Middleware(ref response) => &**response,
		}
	}
}

impl<M: Metadata, S: Middleware<M>> DerefMut for Handler<M, S> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		match *self {
			Handler::Rpc(ref mut handler) => handler,
			Handler::Error(ref mut handler) => handler,
			Handler::Middleware(ref mut response) => &mut **response,
		}
	}
}

pub struct RpcHandler<M: Metadata, S: Middleware<M>> {
	cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
	jsonrpc_handler: Rpc<M, S>,
	metadata: Option<M>,
	control: Control,
	request: Request,
	response: Response,
	/// Asynchronous response waiting to be moved into `response` field.
	waiting_sender: mpsc::Sender<Response>,
	waiting_response: mpsc::Receiver<Response>,
}

impl<M: Metadata, S: Middleware<M>> RpcHandler<M, S> {
	fn response_headers(&self, is_options: bool, cors_header: Option<header::AccessControlAllowOrigin>) -> Headers {
		let mut headers = Headers::new();
		headers.set(self.response.content_type.clone());

		if is_options {
			headers.set(header::Allow(vec![
				Method::Options,
				Method::Post,
			]));
			headers.set(header::Accept(vec![
				header::qitem(mime::Mime(mime::TopLevel::Application, mime::SubLevel::Json, vec![]))
			]));
		}

		if let Some(cors_domain) = cors_header {
			headers.set(header::AccessControlAllowMethods(vec![
				Method::Options,
				Method::Post
			]));
			headers.set(header::AccessControlAllowHeaders(vec![
				UniCase("origin".to_owned()),
				UniCase("content-type".to_owned()),
				UniCase("accept".to_owned()),
			]));
			headers.set(cors_domain);
			headers.set(header::Vary::Items(vec![
				UniCase("origin".to_owned())
			]));
		}

		headers
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

impl<M: Metadata, S: Middleware<M>> server::Handler<HttpStream> for RpcHandler<M, S> {
	fn on_request(&mut self, request: server::Request<HttpStream>) -> Next {
		// Read origin
		let cors_header = utils::cors_header(&request, &self.cors_domains);
		if cors_header == cors::CorsHeader::Invalid {
			self.response = Response::invalid_cors();
			return Next::write();
		}

		// Set request params
		self.request.cors_header = cors_header.into();
		self.request.method = request.method().clone();
		// Read metadata
		self.metadata = Some(self.jsonrpc_handler.extractor.read_metadata(&request));

		match self.request.method {
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
			},
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
				_ => Next::end(),
			}
		}
	}

	/// This event occurs after the first time this handled signals `Next::write()`.
	fn on_response(&mut self, response: &mut server::Response) -> Next {
		// Check if there is a response pending
		if let Ok(output) = self.waiting_response.try_recv() {
			self.response = output;
		}

		let is_options = self.request.method == Method::Options;
		let cors_header = self.request.cors_header.take();
		*response.headers_mut() = self.response_headers(is_options, cors_header);

		self.response.on_response(response)
	}

	/// This event occurs each time the `Response` is ready to be written to.
	fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
		self.response.on_response_writable(encoder)
	}
}

