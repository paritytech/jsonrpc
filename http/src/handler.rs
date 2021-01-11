use crate::WeakRpc;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use std::{fmt, mem, str};

use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{self, service::Service, Body, Method};

use crate::jsonrpc::serde_json;
use crate::jsonrpc::{self as core, middleware, Metadata, Middleware};
use crate::response::Response;
use crate::server_utils::cors;

use crate::{utils, AllowedHosts, CorsDomains, RequestMiddleware, RequestMiddlewareAction, RestApi};

/// jsonrpc http request handler.
pub struct ServerHandler<M: Metadata = (), S: Middleware<M> = middleware::Noop> {
	jsonrpc_handler: WeakRpc<M, S>,
	allowed_hosts: AllowedHosts,
	cors_domains: CorsDomains,
	cors_max_age: Option<u32>,
	cors_allowed_headers: cors::AccessControlAllowHeaders,
	middleware: Arc<dyn RequestMiddleware>,
	rest_api: RestApi,
	health_api: Option<(String, String)>,
	max_request_body_size: usize,
	keep_alive: bool,
}

impl<M: Metadata, S: Middleware<M>> ServerHandler<M, S> {
	/// Create new request handler.
	pub fn new(
		jsonrpc_handler: WeakRpc<M, S>,
		cors_domains: CorsDomains,
		cors_max_age: Option<u32>,
		cors_allowed_headers: cors::AccessControlAllowHeaders,
		allowed_hosts: AllowedHosts,
		middleware: Arc<dyn RequestMiddleware>,
		rest_api: RestApi,
		health_api: Option<(String, String)>,
		max_request_body_size: usize,
		keep_alive: bool,
	) -> Self {
		ServerHandler {
			jsonrpc_handler,
			allowed_hosts,
			cors_domains,
			cors_max_age,
			cors_allowed_headers,
			middleware,
			rest_api,
			health_api,
			max_request_body_size,
			keep_alive,
		}
	}
}

impl<M: Metadata, S: Middleware<M>> Service<hyper::Request<Body>> for ServerHandler<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	type Response = hyper::Response<Body>;
	type Error = hyper::Error;
	type Future = Handler<M, S>;

	fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> task::Poll<hyper::Result<()>> {
		task::Poll::Ready(Ok(()))
	}

	fn call(&mut self, request: hyper::Request<Body>) -> Self::Future {
		let is_host_allowed = utils::is_host_allowed(&request, &self.allowed_hosts);
		let action = self.middleware.on_request(request);

		let (should_validate_hosts, should_continue_on_invalid_cors, response) = match action {
			RequestMiddlewareAction::Proceed {
				should_continue_on_invalid_cors,
				request,
			} => (true, should_continue_on_invalid_cors, Err(request)),
			RequestMiddlewareAction::Respond {
				should_validate_hosts,
				response,
			} => (should_validate_hosts, false, Ok(response)),
		};

		// Validate host
		if should_validate_hosts && !is_host_allowed {
			return Handler::Err(Some(Response::host_not_allowed()));
		}

		// Replace response with the one returned by middleware.
		match response {
			Ok(response) => Handler::Middleware(response),
			Err(request) => {
				Handler::Rpc(RpcHandler {
					jsonrpc_handler: self.jsonrpc_handler.clone(),
					state: RpcHandlerState::ReadingHeaders {
						request,
						cors_domains: self.cors_domains.clone(),
						cors_headers: self.cors_allowed_headers.clone(),
						continue_on_invalid_cors: should_continue_on_invalid_cors,
						keep_alive: self.keep_alive,
					},
					is_options: false,
					cors_max_age: self.cors_max_age,
					cors_allow_origin: cors::AllowCors::NotRequired,
					cors_allow_headers: cors::AllowCors::NotRequired,
					rest_api: self.rest_api,
					health_api: self.health_api.clone(),
					max_request_body_size: self.max_request_body_size,
					// initial value, overwritten when reading client headers
					keep_alive: true,
				})
			}
		}
	}
}

pub enum Handler<M: Metadata, S: Middleware<M>> {
	Rpc(RpcHandler<M, S>),
	Err(Option<Response>),
	Middleware(Pin<Box<dyn Future<Output = hyper::Result<hyper::Response<Body>>> + Send>>),
}

impl<M: Metadata, S: Middleware<M>> Future for Handler<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	type Output = hyper::Result<hyper::Response<Body>>;

	fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
		match Pin::into_inner(self) {
			Handler::Rpc(ref mut handler) => Pin::new(handler).poll(cx),
			Handler::Middleware(ref mut middleware) => Pin::new(middleware).poll(cx),
			Handler::Err(ref mut response) => Poll::Ready(Ok(response
				.take()
				.expect("Response always Some initialy. Returning `Ready` so will never be polled again; qed")
				.into())),
		}
	}
}

enum RpcPollState<M> {
	Ready(RpcHandlerState<M>),
	NotReady(RpcHandlerState<M>),
}

impl<M> RpcPollState<M> {
	fn decompose(self) -> (RpcHandlerState<M>, bool) {
		use self::RpcPollState::*;
		match self {
			Ready(handler) => (handler, true),
			NotReady(handler) => (handler, false),
		}
	}
}

enum RpcHandlerState<M> {
	ReadingHeaders {
		request: hyper::Request<Body>,
		cors_domains: CorsDomains,
		cors_headers: cors::AccessControlAllowHeaders,
		continue_on_invalid_cors: bool,
		keep_alive: bool,
	},
	ReadingBody {
		body: hyper::Body,
		uri: Option<hyper::Uri>,
		request: Vec<u8>,
		metadata: M,
	},
	ProcessRest {
		uri: hyper::Uri,
		metadata: M,
	},
	ProcessHealth {
		method: String,
		metadata: M,
	},
	Writing(Response),
	Waiting(Pin<Box<dyn Future<Output = Option<String>> + Send>>),
	WaitingForResponse(Pin<Box<dyn Future<Output = Response> + Send>>),
	Done,
}

impl<M> fmt::Debug for RpcHandlerState<M> {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		use self::RpcHandlerState::*;

		match *self {
			ReadingHeaders { .. } => write!(fmt, "ReadingHeaders"),
			ReadingBody { .. } => write!(fmt, "ReadingBody"),
			ProcessRest { .. } => write!(fmt, "ProcessRest"),
			ProcessHealth { .. } => write!(fmt, "ProcessHealth"),
			Writing(ref res) => write!(fmt, "Writing({:?})", res),
			WaitingForResponse(_) => write!(fmt, "WaitingForResponse"),
			Waiting(_) => write!(fmt, "Waiting"),
			Done => write!(fmt, "Done"),
		}
	}
}

pub struct RpcHandler<M: Metadata, S: Middleware<M>> {
	jsonrpc_handler: WeakRpc<M, S>,
	state: RpcHandlerState<M>,
	is_options: bool,
	cors_allow_origin: cors::AllowCors<header::HeaderValue>,
	cors_allow_headers: cors::AllowCors<Vec<header::HeaderValue>>,
	cors_max_age: Option<u32>,
	rest_api: RestApi,
	health_api: Option<(String, String)>,
	max_request_body_size: usize,
	keep_alive: bool,
}

impl<M: Metadata, S: Middleware<M>> Future for RpcHandler<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
	M: Unpin,
{
	type Output = hyper::Result<hyper::Response<Body>>;

	fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
		let this = Pin::into_inner(self);

		let new_state = match mem::replace(&mut this.state, RpcHandlerState::Done) {
			RpcHandlerState::ReadingHeaders {
				request,
				cors_domains,
				cors_headers,
				continue_on_invalid_cors,
				keep_alive,
			} => {
				// Read cors header
				this.cors_allow_origin = utils::cors_allow_origin(&request, &cors_domains);
				this.cors_allow_headers = utils::cors_allow_headers(&request, &cors_headers);
				this.keep_alive = utils::keep_alive(&request, keep_alive);
				this.is_options = *request.method() == Method::OPTIONS;
				// Read other headers
				RpcPollState::Ready(this.read_headers(request, continue_on_invalid_cors))
			}
			RpcHandlerState::ReadingBody {
				body,
				request,
				metadata,
				uri,
			} => match this.process_body(body, request, uri, metadata, cx) {
				Err(BodyError::Utf8(ref e)) => {
					let mesg = format!("utf-8 encoding error at byte {} in request body", e.valid_up_to());
					let resp = Response::bad_request(mesg);
					RpcPollState::Ready(RpcHandlerState::Writing(resp))
				}
				Err(BodyError::TooLarge) => {
					let resp = Response::too_large("request body size exceeds allowed maximum");
					RpcPollState::Ready(RpcHandlerState::Writing(resp))
				}
				Err(BodyError::Hyper(e)) => return Poll::Ready(Err(e)),
				Ok(state) => state,
			},
			RpcHandlerState::ProcessRest { uri, metadata } => this.process_rest(uri, metadata)?,
			RpcHandlerState::ProcessHealth { method, metadata } => this.process_health(method, metadata)?,
			RpcHandlerState::WaitingForResponse(mut waiting) => match Pin::new(&mut waiting).poll(cx) {
				Poll::Ready(response) => RpcPollState::Ready(RpcHandlerState::Writing(response)),
				Poll::Pending => RpcPollState::NotReady(RpcHandlerState::WaitingForResponse(waiting)),
			},
			RpcHandlerState::Waiting(mut waiting) => {
				match Pin::new(&mut waiting).poll(cx) {
					Poll::Ready(response) => {
						RpcPollState::Ready(RpcHandlerState::Writing(match response {
							// Notification, just return empty response.
							None => Response::ok(String::new()),
							// Add new line to have nice output when using CLI clients (curl)
							Some(result) => Response::ok(format!("{}\n", result)),
						}))
					}
					Poll::Pending => RpcPollState::NotReady(RpcHandlerState::Waiting(waiting)),
				}
			}
			state => RpcPollState::NotReady(state),
		};

		let (new_state, is_ready) = new_state.decompose();
		match new_state {
			RpcHandlerState::Writing(res) => {
				let mut response: hyper::Response<Body> = res.into();
				let cors_allow_origin = mem::replace(&mut this.cors_allow_origin, cors::AllowCors::Invalid);
				let cors_allow_headers = mem::replace(&mut this.cors_allow_headers, cors::AllowCors::Invalid);

				Self::set_response_headers(
					response.headers_mut(),
					this.is_options,
					this.cors_max_age,
					cors_allow_origin.into(),
					cors_allow_headers.into(),
					this.keep_alive,
				);
				Poll::Ready(Ok(response))
			}
			state => {
				this.state = state;
				if is_ready {
					Pin::new(this).poll(cx)
				} else {
					Poll::Pending
				}
			}
		}
	}
}

// Intermediate and internal error type to better distinguish
// error cases occurring during request body processing.
enum BodyError {
	Hyper(hyper::Error),
	Utf8(str::Utf8Error),
	TooLarge,
}

impl From<hyper::Error> for BodyError {
	fn from(e: hyper::Error) -> BodyError {
		BodyError::Hyper(e)
	}
}

impl<M: Metadata, S: Middleware<M>> RpcHandler<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	fn read_headers(&self, request: hyper::Request<Body>, continue_on_invalid_cors: bool) -> RpcHandlerState<M> {
		if self.cors_allow_origin == cors::AllowCors::Invalid && !continue_on_invalid_cors {
			return RpcHandlerState::Writing(Response::invalid_allow_origin());
		}

		if self.cors_allow_headers == cors::AllowCors::Invalid && !continue_on_invalid_cors {
			return RpcHandlerState::Writing(Response::invalid_allow_headers());
		}

		// Read metadata
		let handler = match self.jsonrpc_handler.upgrade() {
			Some(handler) => handler,
			None => return RpcHandlerState::Writing(Response::closing()),
		};
		let metadata = handler.extractor.read_metadata(&request);

		// Proceed
		match *request.method() {
			// Validate the ContentType header
			// to prevent Cross-Origin XHRs with text/plain
			Method::POST if Self::is_json(request.headers().get("content-type")) => {
				let uri = if self.rest_api != RestApi::Disabled {
					Some(request.uri().clone())
				} else {
					None
				};
				RpcHandlerState::ReadingBody {
					metadata,
					request: Default::default(),
					uri,
					body: request.into_body(),
				}
			}
			Method::POST if self.rest_api == RestApi::Unsecure && request.uri().path().split('/').count() > 2 => {
				RpcHandlerState::ProcessRest {
					metadata,
					uri: request.uri().clone(),
				}
			}
			// Just return error for unsupported content type
			Method::POST => RpcHandlerState::Writing(Response::unsupported_content_type()),
			// Don't validate content type on options
			Method::OPTIONS => RpcHandlerState::Writing(Response::empty()),
			// Respond to health API request if there is one configured.
			Method::GET if self.health_api.as_ref().map(|x| &*x.0) == Some(request.uri().path()) => {
				RpcHandlerState::ProcessHealth {
					metadata,
					method: self
						.health_api
						.as_ref()
						.map(|x| x.1.clone())
						.expect("Health api is defined since the URI matched."),
				}
			}
			// Disallow other methods.
			_ => RpcHandlerState::Writing(Response::method_not_allowed()),
		}
	}

	fn process_health(&self, method: String, metadata: M) -> Result<RpcPollState<M>, hyper::Error> {
		use self::core::types::{Call, Failure, Id, MethodCall, Output, Params, Request, Success, Version};

		// Create a request
		let call = Request::Single(Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method,
			params: Params::None,
			id: Id::Num(1),
		}));

		let response = match self.jsonrpc_handler.upgrade() {
			Some(h) => h.handler.handle_rpc_request(call, metadata),
			None => return Ok(RpcPollState::Ready(RpcHandlerState::Writing(Response::closing()))),
		};

		Ok(RpcPollState::Ready(RpcHandlerState::WaitingForResponse(Box::pin(
			async {
				match response.await {
					Some(core::Response::Single(Output::Success(Success { result, .. }))) => {
						let result = serde_json::to_string(&result).expect("Serialization of result is infallible;qed");

						Response::ok(result)
					}
					Some(core::Response::Single(Output::Failure(Failure { error, .. }))) => {
						let result = serde_json::to_string(&error).expect("Serialization of error is infallible;qed");

						Response::service_unavailable(result)
					}
					e => Response::internal_error(format!("Invalid response for health request: {:?}", e)),
				}
			},
		))))
	}

	fn process_rest(&self, uri: hyper::Uri, metadata: M) -> Result<RpcPollState<M>, hyper::Error> {
		use self::core::types::{Call, Id, MethodCall, Params, Request, Value, Version};

		// skip the initial /
		let mut it = uri.path().split('/').skip(1);

		// parse method & params
		let method = it.next().unwrap_or("");
		let mut params = Vec::new();
		for param in it {
			let v = serde_json::from_str(param)
				.or_else(|_| serde_json::from_str(&format!("\"{}\"", param)))
				.unwrap_or(Value::Null);
			params.push(v)
		}

		// Create a request
		let call = Request::Single(Call::MethodCall(MethodCall {
			jsonrpc: Some(Version::V2),
			method: method.into(),
			params: Params::Array(params),
			id: Id::Num(1),
		}));

		let response = match self.jsonrpc_handler.upgrade() {
			Some(h) => h.handler.handle_rpc_request(call, metadata),
			None => return Ok(RpcPollState::Ready(RpcHandlerState::Writing(Response::closing()))),
		};

		Ok(RpcPollState::Ready(RpcHandlerState::Waiting(Box::pin(async {
			response
				.await
				.map(|x| serde_json::to_string(&x).expect("Serialization of response is infallible;qed"))
		}))))
	}

	fn process_body(
		&self,
		mut body: hyper::Body,
		mut request: Vec<u8>,
		uri: Option<hyper::Uri>,
		metadata: M,
		cx: &mut task::Context<'_>,
	) -> Result<RpcPollState<M>, BodyError> {
		use futures::Stream;

		loop {
			let pinned_body = Pin::new(&mut body);
			match pinned_body.poll_next(cx)? {
				Poll::Ready(Some(chunk)) => {
					if request
						.len()
						.checked_add(chunk.len())
						.map(|n| n > self.max_request_body_size)
						.unwrap_or(true)
					{
						return Err(BodyError::TooLarge);
					}
					request.extend_from_slice(&*chunk)
				}
				Poll::Ready(None) => {
					if let (Some(uri), true) = (uri, request.is_empty()) {
						return Ok(RpcPollState::Ready(RpcHandlerState::ProcessRest { uri, metadata }));
					}

					let content = match str::from_utf8(&request) {
						Ok(content) => content,
						Err(err) => {
							// Return utf error.
							return Err(BodyError::Utf8(err));
						}
					};

					let response = match self.jsonrpc_handler.upgrade() {
						Some(h) => h.handler.handle_request(content, metadata),
						None => return Ok(RpcPollState::Ready(RpcHandlerState::Writing(Response::closing()))),
					};

					// Content is ready
					return Ok(RpcPollState::Ready(RpcHandlerState::Waiting(Box::pin(response))));
				}
				Poll::Pending => {
					return Ok(RpcPollState::NotReady(RpcHandlerState::ReadingBody {
						body,
						request,
						metadata,
						uri,
					}));
				}
			}
		}
	}

	fn set_response_headers(
		headers: &mut HeaderMap,
		is_options: bool,
		cors_max_age: Option<u32>,
		cors_allow_origin: Option<HeaderValue>,
		cors_allow_headers: Option<Vec<HeaderValue>>,
		keep_alive: bool,
	) {
		let as_header = |m: Method| m.as_str().parse().expect("`Method` will always parse; qed");
		let concat = |headers: &[HeaderValue]| {
			let separator = b", ";
			let val = headers
				.iter()
				.flat_map(|h| h.as_bytes().iter().chain(separator.iter()))
				.cloned()
				.collect::<Vec<_>>();
			let max_len = if val.is_empty() { 0 } else { val.len() - 2 };
			HeaderValue::from_bytes(&val[..max_len])
				.expect("Concatenation of valid headers with `, ` is still valid; qed")
		};

		let allowed = concat(&[as_header(Method::OPTIONS), as_header(Method::POST)]);

		if is_options {
			headers.append(header::ALLOW, allowed.clone());
			headers.append(header::ACCEPT, HeaderValue::from_static("application/json"));
		}

		if let Some(cors_allow_origin) = cors_allow_origin {
			headers.append(header::VARY, HeaderValue::from_static("origin"));
			headers.append(header::ACCESS_CONTROL_ALLOW_METHODS, allowed);
			headers.append(header::ACCESS_CONTROL_ALLOW_ORIGIN, cors_allow_origin);

			if let Some(cma) = cors_max_age {
				headers.append(
					header::ACCESS_CONTROL_MAX_AGE,
					HeaderValue::from_str(&cma.to_string()).expect("`u32` will always parse; qed"),
				);
			}

			if let Some(cors_allow_headers) = cors_allow_headers {
				if !cors_allow_headers.is_empty() {
					headers.append(header::ACCESS_CONTROL_ALLOW_HEADERS, concat(&cors_allow_headers));
				}
			}
		}

		if !keep_alive {
			headers.append(header::CONNECTION, HeaderValue::from_static("close"));
		}
	}

	/// Returns true if the `content_type` header indicates a valid JSON
	/// message.
	fn is_json(content_type: Option<&header::HeaderValue>) -> bool {
		match content_type.and_then(|val| val.to_str().ok()) {
			Some(ref content)
				if content.eq_ignore_ascii_case("application/json")
					|| content.eq_ignore_ascii_case("application/json; charset=utf-8")
					|| content.eq_ignore_ascii_case("application/json;charset=utf-8") =>
			{
				true
			}
			_ => false,
		}
	}
}

#[cfg(test)]
mod test {
	use super::{hyper, RpcHandler};
	use jsonrpc_core::middleware::Noop;

	#[test]
	fn test_case_insensitive_content_type() {
		let request = hyper::Request::builder()
			.header("content-type", "Application/Json; charset=UTF-8")
			.body(())
			.unwrap();

		let request2 = hyper::Request::builder()
			.header("content-type", "Application/Json;charset=UTF-8")
			.body(())
			.unwrap();

		assert_eq!(
			request.headers().get("content-type").unwrap(),
			&"Application/Json; charset=UTF-8"
		);

		assert_eq!(
			RpcHandler::<(), Noop>::is_json(request.headers().get("content-type")),
			true
		);
		assert_eq!(
			RpcHandler::<(), Noop>::is_json(request2.headers().get("content-type")),
			true
		);
	}
}
