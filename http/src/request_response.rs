//! Basic Request/Response structures used internally.

use std::io;
use hyper::{net, server, Next, Encoder, Decoder};

pub use hyper::status::StatusCode;
pub use hyper::header;

/// Simple client Request structure
pub struct Request {
	/// Request content
	pub content: String,
	/// CORS header to respond with
	pub cors_header: Option<header::AccessControlAllowOrigin>,
}

impl Request {
	/// Create empty `Request`
	pub fn empty() -> Self {
		Request {
			content: String::new(),
			cors_header: None,
		}
	}
}

/// Simple server response structure
pub struct Response {
	/// Response code
	pub code: StatusCode,
	/// Response content type
	pub content_type: header::ContentType,
	/// Response body
	pub content: String,
	/// Number of bytes already written
	pub write_pos: usize,
}

impl Response {
	/// Create response with empty body and 200 OK status code.
	pub fn empty() -> Self {
		Self::ok(String::new())
	}

	/// Create response with given body and 200 OK status code.
	pub fn ok<T: Into<String>>(response: T) -> Self {
		Response {
			code: StatusCode::Ok,
			content_type: header::ContentType::json(),
			content: response.into(),
			write_pos: 0,
		}
	}

	/// Create response for not allowed hosts.
	pub fn host_not_allowed() -> Self {
		Response {
			code: StatusCode::Forbidden,
			content_type: header::ContentType::html(),
			content: "Provided Host header is not whitelisted.\n".to_owned(),
			write_pos: 0,
		}
	}

	/// Create response for unsupported content type.
	pub fn unsupported_content_type() -> Self {
		Response {
			code: StatusCode::UnsupportedMediaType,
			content_type: header::ContentType::html(),
			content: "Supplied content type is not allowed. Content-Type: application/json is required\n".to_owned(),
			write_pos: 0,
		}
	}

	/// Create response for disallowed method used.
	pub fn method_not_allowed() -> Self {
		Response {
			code: StatusCode::MethodNotAllowed,
			content_type: header::ContentType::html(),
			content: "Used HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned(),
			write_pos: 0,
		}
	}
}
impl server::Handler<net::HttpStream> for Response {
	fn on_request(&mut self, _request: server::Request<net::HttpStream>) -> Next {
		Next::write()
	}

	fn on_request_readable(&mut self, _decoder: &mut Decoder<net::HttpStream>) -> Next {
		Next::write()
	}

	fn on_response(&mut self, response: &mut server::Response) -> Next {
		response.set_status(self.code);
		Next::write()
	}

	/// This event occurs each time the `Response` is ready to be written to.
	fn on_response_writable(&mut self, encoder: &mut Encoder<net::HttpStream>) -> Next {
		let bytes = self.content.as_bytes();
		if bytes.len() == self.write_pos {
			return Next::end();
		}

		match encoder.write(&bytes[self.write_pos..]) {
			Ok(0) => {
				Next::write()
			}
			Ok(bytes) => {
				self.write_pos += bytes;
				Next::write()
			}
			Err(e) => match e.kind() {
				io::ErrorKind::WouldBlock => Next::write(),
				_ => Next::end(),
			}
		}
	}
}

