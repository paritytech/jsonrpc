use Rpc;

use std::{fmt, mem};
use std::sync::Arc;

use hyper::{self, mime, server, Method};
use hyper::header::{self, Headers};
use unicase::Ascii;

use jsonrpc::{self as core, FutureResult, BoxFuture, Metadata, Middleware, NoopMiddleware};
use jsonrpc::futures::{Future, Poll, Async, Stream};
use response::Response;
use server_utils::cors;

use {utils, RequestMiddleware, RequestMiddlewareAction, CorsDomains, AllowedHosts};

/// jsonrpc http request handler.
pub struct ServerHandler<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	jsonrpc_handler: Rpc<M, S>,
	allowed_hosts: AllowedHosts,
	cors_domains: CorsDomains,
	middleware: Arc<RequestMiddleware>,
}

impl<M: Metadata, S: Middleware<M>> ServerHandler<M, S> {
	/// Create new request handler.
	pub fn new(
		jsonrpc_handler: Rpc<M, S>,
		cors_domains: CorsDomains,
		allowed_hosts: AllowedHosts,
		middleware: Arc<RequestMiddleware>,
	) -> Self {
		ServerHandler {
			jsonrpc_handler: jsonrpc_handler,
			allowed_hosts: allowed_hosts,
			cors_domains: cors_domains,
			middleware: middleware,
		}
	}
}

impl<M: Metadata, S: Middleware<M>> server::Service for ServerHandler<M, S> {
	type Request = server::Request;
	type Response = server::Response;
	type Error = hyper::Error;
	type Future = Handler<M, S>;

	fn call(&self, request: Self::Request) -> Self::Future {
		enum MiddlewareResponse {
			Some(BoxFuture<server::Response, hyper::Error>),
			None(server::Request),
		}

		let is_host_allowed = utils::is_host_allowed(&request, &self.allowed_hosts);
		let action = self.middleware.on_request(request);

		let (should_validate_hosts, should_continue_on_invalid_cors, response) = match action {
			RequestMiddlewareAction::Proceed { should_continue_on_invalid_cors, request }=> (
				true, should_continue_on_invalid_cors, MiddlewareResponse::None(request)
			),
			RequestMiddlewareAction::Respond { should_validate_hosts, response } => (
				should_validate_hosts, false, MiddlewareResponse::Some(response)
			),
		};

		// Validate host
		if should_validate_hosts && !is_host_allowed {
			return Handler::Error(Some(Response::host_not_allowed()));
		}

		// Replace response with the one returned by middleware.
		match response {
			MiddlewareResponse::Some(response) => Handler::Middleware(response),
			MiddlewareResponse::None(request) => {
				Handler::Rpc(RpcHandler {
					jsonrpc_handler: self.jsonrpc_handler.clone(),
					state: RpcHandlerState::ReadingHeaders {
						request: request,
						cors_domains: self.cors_domains.clone(),
						continue_on_invalid_cors: should_continue_on_invalid_cors,
					},
					is_options: false,
					cors_header: cors::CorsHeader::NotRequired,
				})
			}
		}
	}
}

pub enum Handler<M: Metadata, S: Middleware<M>> {
	Rpc(RpcHandler<M, S>),
	Error(Option<Response>),
	Middleware(BoxFuture<server::Response, hyper::Error>),
}

impl<M: Metadata, S: Middleware<M>> Future for Handler<M, S> {
	type Item = server::Response;
	type Error = hyper::Error;

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		match *self {
			Handler::Rpc(ref mut handler) => handler.poll(),
			Handler::Middleware(ref mut middleware) => middleware.poll(),
			Handler::Error(ref mut response) => Ok(Async::Ready(
				response.take().expect("Response always Some initialy. Returning `Ready` so will never be polled again; qed").into()
			)),
		}
	}
}

enum RpcPollState<M, F> where
	F: Future<Item = Option<core::Response>, Error = ()>,
{
	Ready(RpcHandlerState<M, F>),
	NotReady(RpcHandlerState<M, F>),
}

impl<M, F> RpcPollState<M, F> where
	F: Future<Item = Option<core::Response>, Error = ()>,
{
	fn decompose(self) -> (RpcHandlerState<M, F>, bool) {
		use self::RpcPollState::*;
		match self {
			Ready(handler) => (handler, true),
			NotReady(handler) => (handler, false),
		}
	}
}

enum RpcHandlerState<M, F> where
	F: Future<Item = Option<core::Response>, Error = ()>,
{
	ReadingHeaders {
		request: server::Request,
		cors_domains: CorsDomains,
		continue_on_invalid_cors: bool,
	},
	ReadingBody {
		body: hyper::Body,
		request: Vec<u8>,
		metadata: M,
	},
	Writing(Response),
	Waiting(FutureResult<F>),
	Done,
}

impl<M, F> fmt::Debug for RpcHandlerState<M, F> where
	F: Future<Item = Option<core::Response>, Error = ()>,
{
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		use self::RpcHandlerState::*;

		match *self {
			ReadingHeaders {..} => write!(fmt, "ReadingHeaders"),
			ReadingBody {..} => write!(fmt, "ReadingBody"),
			Writing(ref res) => write!(fmt, "Writing({:?})", res),
			Waiting(_) => write!(fmt, "Waiting"),
			Done => write!(fmt, "Done"),
		}
	}
}

pub struct RpcHandler<M: Metadata, S: Middleware<M>> {
	jsonrpc_handler: Rpc<M, S>,
	state: RpcHandlerState<M, S::Future>,
	is_options: bool,
	cors_header: cors::CorsHeader<header::AccessControlAllowOrigin>,
}

impl<M: Metadata, S: Middleware<M>> Future for RpcHandler<M, S> {
	type Item = server::Response;
	type Error = hyper::Error;

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		let new_state = match mem::replace(&mut self.state, RpcHandlerState::Done) {
			RpcHandlerState::ReadingHeaders { request, cors_domains, continue_on_invalid_cors, } => {
				// Read cors header
				self.cors_header = utils::cors_header(&request, &cors_domains);
				self.is_options = *request.method() == Method::Options;
				// Read other headers
				RpcPollState::Ready(self.read_headers(request, continue_on_invalid_cors))
			},
			RpcHandlerState::ReadingBody { body, request, metadata, } => {
				self.process_body(body, request, metadata)?
			},
			RpcHandlerState::Waiting(mut waiting) => {
				match waiting.poll() {
					Ok(Async::Ready(response)) => {
						RpcPollState::Ready(RpcHandlerState::Writing(match response {
							// Notification, just return empty response.
							None => Response::ok(String::new()),
							// Add new line to have nice output when using CLI clients (curl)
							Some(result) => Response::ok(format!("{}\n", result)),
						}.into()))
					},
					Ok(Async::NotReady) => RpcPollState::NotReady(RpcHandlerState::Waiting(waiting)),
					Err(_) => RpcPollState::Ready(RpcHandlerState::Writing(Response::internal_error())),
				}
			},
			state => RpcPollState::NotReady(state),
		};

		let (new_state, is_ready) = new_state.decompose();
		match new_state {
			RpcHandlerState::Writing(res) => {
				let mut response: server::Response = res.into();
				let cors_header = mem::replace(&mut self.cors_header, cors::CorsHeader::Invalid);
				Self::set_response_headers(response.headers_mut(), self.is_options, cors_header.into());
				Ok(Async::Ready(response))
			},
			state => {
				self.state = state;
				if is_ready {
					self.poll()
				} else {
					Ok(Async::NotReady)
				}
			},
		}
	}
}

impl<M: Metadata, S: Middleware<M>> RpcHandler<M, S> {
	fn read_headers(
		&self,
		request: server::Request,
		continue_on_invalid_cors: bool,
	) -> RpcHandlerState<M, S::Future> {
		if self.cors_header == cors::CorsHeader::Invalid && !continue_on_invalid_cors {
			return RpcHandlerState::Writing(Response::invalid_cors());
		}
		// Read metadata
		let metadata = self.jsonrpc_handler.extractor.read_metadata(&request);

		// Proceed
		match *request.method() {
			// Validate the ContentType header
			// to prevent Cross-Origin XHRs with text/plain
			Method::Post if Self::is_json(request.headers().get::<header::ContentType>()) => {
				RpcHandlerState::ReadingBody {
					metadata: metadata,
					request: Default::default(),
					body: request.body(),
				}
			},
			// Just return error for unsupported content type
			Method::Post => {
				RpcHandlerState::Writing(Response::unsupported_content_type())
			},
			// Don't validate content type on options
			Method::Options => {
				RpcHandlerState::Writing(Response::empty())
			},
			// Disallow other methods.
			_ => {
				RpcHandlerState::Writing(Response::method_not_allowed())
			},
		}
	}

	fn process_body(
		&self,
		mut body: hyper::Body,
		mut request: Vec<u8>,
		metadata: M,
	) -> Result<RpcPollState<M, S::Future>, hyper::Error> {
		loop {
			match body.poll()? {
				// TODO [ToDr] reject too large requests?
				Async::Ready(Some(chunk)) => {
					request.extend_from_slice(&*chunk)
				},
				Async::Ready(None) => {
					let content = match ::std::str::from_utf8(&request) {
						Ok(content) => content,
						Err(err) => {
							// Return utf error.
							return Err(hyper::Error::Utf8(err));
						},
					};

					// Content is ready
					return Ok(RpcPollState::Ready(RpcHandlerState::Waiting(
						self.jsonrpc_handler.handler.handle_request(content, metadata)
					)));
				},
				Async::NotReady => {
					return Ok(RpcPollState::NotReady(RpcHandlerState::ReadingBody {
						body: body,
						request: request,
						metadata: metadata
					}));
				},
			}
		}
	}

	fn set_response_headers(headers: &mut Headers, is_options: bool, cors_header: Option<header::AccessControlAllowOrigin>) {
		if is_options {
			headers.set(header::Allow(vec![
				Method::Options,
				Method::Post,
			]));
			headers.set(header::Accept(vec![
				header::qitem(mime::APPLICATION_JSON)
			]));
		}

		if let Some(cors_domain) = cors_header {
			headers.set(header::AccessControlAllowMethods(vec![
				Method::Options,
				Method::Post
			]));
			headers.set(header::AccessControlAllowHeaders(vec![
				Ascii::new("origin".to_owned()),
				Ascii::new("content-type".to_owned()),
				Ascii::new("accept".to_owned()),
			]));
			headers.set(cors_domain);
			headers.set(header::Vary::Items(vec![
				Ascii::new("origin".to_owned())
			]));
		}
	}

	fn is_json(content_type: Option<&header::ContentType>) -> bool {
		const APPLICATION_JSON_UTF_8: &str = "application/json; charset=utf-8";

		match content_type {
			Some(&header::ContentType(ref mime))
				if *mime == mime::APPLICATION_JSON || *mime == APPLICATION_JSON_UTF_8 => true,
			_ => false
		}
	}
}
