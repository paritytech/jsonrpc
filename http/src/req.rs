//! Convenient Request wrapper used internally.

use std::ascii::AsciiExt;
use tokio_minihttp;
use tokio_core::io::EasyBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
	Post,
	Options,
	Other
}

pub struct Req {
	request: tokio_minihttp::Request,
	body: EasyBuf,
}

impl Req {

	/// Creates new `Req` object
	pub fn new(request: tokio_minihttp::Request) -> Self {
		let body = request.body();
		Req {
			request: request,
			body: body,
		}
	}

	/// Returns request method
	pub fn method(&self) -> Method {
		// RFC 2616: The method is case-sensitive
		match self.request.method() {
			"OPTIONS" => Method::Options,
			"POST" => Method::Post,
			_ => Method::Other,
		}
	}

	/// Returns value of first header with given name.
	/// `None` if header is not found or value is not utf-8 encoded
	pub fn header(&self, name: &str) -> Option<&str> {
		self.request.headers()
			.find(|header| header.0.eq_ignore_ascii_case(name))
			.and_then(|header| ::std::str::from_utf8(header.1).ok())
	}

	pub fn body(&self) -> &str {
		::std::str::from_utf8(self.body.as_slice()).unwrap_or("")
	}
}
