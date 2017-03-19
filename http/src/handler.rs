use std::{fmt, io};
use std::sync::Arc;
use futures::{Future, future};
use hyper::server::{Handler, Request, Response};
use hyper::status::StatusCode;
use hyper::method::Method;
use hyper::{mime, header};
use tokio_io::AsyncRead;
use tokio_io::io::read_to_end;
use jsonrpc;
use jsonrpc_server_utils::{cors, hosts};

use {utils, RequestMiddleware, RequestMiddlewareAction, Rpc};

#[derive(Debug)]
enum Error {
	MiddlewareFailure,
	HostNotAllowed,
	UnsupportedContentType,
	MethodNotAllowed,
	InvalidCors,
	Other,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let s = match *self {
			Error::MiddlewareFailure => "Middleware failure.\n",
			Error::HostNotAllowed => "Provided Host header is not whitelisted.\n",
			Error::UnsupportedContentType => "Supplied content type is not allowed. Content-Type: application/json is required\n",
			Error::MethodNotAllowed => "Used HTTP Method is not allowed. POST or OPTIONS is required\n",
			Error::InvalidCors => "Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n",
			Error::Other => "Other.\n",
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
			Error::Other => StatusCode::Forbidden,
		}
	}
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

//struct RequestRead<'a, 'b: 'a>(&'a Request<'a, 'b>);

//impl<'a, 'b> io::Read for RequestRead<'a, 'b> {
	//fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		//self.0.read(buf)
	//}
//}

//impl<'a, 'b> AsyncRead for RequestRead<'a, 'b> {}

/// jsonrpc http request handler.
pub struct ServerHandler <M: jsonrpc::Metadata = (), S: jsonrpc::Middleware<M> = jsonrpc::NoopMiddleware> {
	pub jsonrpc_handler: Rpc<M, S>,
	pub cors_domains: Option<Vec<cors::AccessControlAllowOrigin>>,
	pub allowed_hosts: Option<Vec<hosts::Host>>,
	pub middleware: Arc<RequestMiddleware>,
}

impl<M: jsonrpc::Metadata, S: jsonrpc::Middleware<M>> Handler for ServerHandler<M, S> {
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
					//return Err(Error::HostNotAllowed);
					return future::err(Error::HostNotAllowed).boxed()
				}

				let cors_header = utils::cors_header(&request, &self.cors_domains);
				if cors_header == cors::CorsHeader::Invalid && !should_continue_on_invalid_cors {
					return future::err(Error::InvalidCors).boxed()
					//return Err(Error::InvalidCors);
				}

				let metadata = self.jsonrpc_handler.extractor.read_metadata(&request);

				match request.method {
					Method::Post if is_json(request.headers.get::<header::ContentType>()) => {
						// TODO: handle jsonrpc request here
						//read_to_end(RequestRead(&request), Vec::new())
							//.map_err(|_err| Error::Other)
							//.and_then(move |(request, read)| {
								//Ok(String::new())
							//}).boxed()
							////.and_then(
						//Ok(String::new())
						future::ok(String::new()).boxed()
					},
					Method::Post => {
						future::err(Error::UnsupportedContentType).boxed()
					},
					Method::Options => {
						future::ok(String::new()).boxed()
					},
					_ => {
						future::err(Error::MethodNotAllowed).boxed()
					}
				}
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

