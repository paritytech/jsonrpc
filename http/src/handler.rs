use std::fmt;
use std::sync::Arc;
use futures::Future;
use hyper::server::{Handler, Request, Response};
use hyper::status::StatusCode;
use hyper::header;
use jsonrpc_server_utils::{hosts};

use {utils, RequestMiddleware, RequestMiddlewareAction};

#[derive(Debug)]
enum Error {
	MiddlewareFailure,
	HostNotAllowed,
	UnsupportedContentType,
	MethodNotAllowed,
	InvalidCors,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let s = match *self {
			Error::MiddlewareFailure => "Middleware failure.\n",
			Error::HostNotAllowed => "Provided Host header is not whitelisted.\n",
			Error::UnsupportedContentType => "Supplied content type is not allowed. Content-Type: application/json is required\n",
			Error::MethodNotAllowed => "Used HTTP Method is not allowed. POST or OPTIONS is required\n",
			Error::InvalidCors => "Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n",
		};

		f.write_str(s)
	}
}

impl Error {
	fn status_code(&self) -> StatusCode {
		match *self {
			Error::MiddlewareFailure => StatusCode::Forbidden,
			Error::HostNotAllowed => StatusCode::Forbidden,
			Error::UnsupportedContentType => StatusCode::UnsupportedMediaType,
			Error::MethodNotAllowed => StatusCode::MethodNotAllowed,
			Error::InvalidCors => StatusCode::Forbidden,
		}
	}
}

/// jsonrpc http request handler.
pub struct ServerHandler {
	pub allowed_hosts: Option<Vec<hosts::Host>>,
	pub middleware: Arc<RequestMiddleware>,
}

impl Handler for ServerHandler {
	fn handle(&self, request: Request, mut response: Response) {
		let handle_future = self.middleware.on_request(&request)
			.map_err(|_err| Error::MiddlewareFailure)
			.and_then(move |action| {
				let (should_validate_hosts, should_continue_on_invalid_cors, handler) = match action {
					RequestMiddlewareAction::Proceed { should_continue_on_invalid_cors } =>
						(true, should_continue_on_invalid_cors, None),
					RequestMiddlewareAction::Respond { should_validate_hosts, handler } =>
						(should_validate_hosts, false, Some(handler))
				};

				if should_validate_hosts && !utils::is_host_allowed(&request, &self.allowed_hosts) {
					// response not allowed
					return Err(Error::HostNotAllowed);
				}

				Ok(())
			})
			.then(move |result| match result {
				Ok(_to_send) => {
					*response.status_mut() = StatusCode::Ok;
					response.headers_mut().set(header::ContentType::json());
					// TODO: here use futures to write response
					Ok(())
				},
				Err(err) => {
					*response.status_mut() = err.status_code();
					response.headers_mut().set(header::ContentType::plaintext());
					response.send(err.to_string().as_bytes())
				},
			});
	}
}

