//! Basic Request/Response structures used internally.

pub use hyper::{self, header::HeaderValue, Body, Method, StatusCode};

/// Simple server response structure
#[derive(Debug)]
pub struct Response {
	/// Response code
	pub code: StatusCode,
	/// Response content type
	pub content_type: HeaderValue,
	/// Response body
	pub content: String,
}

impl Response {
	/// Create a response with empty body and 200 OK status code.
	pub fn empty() -> Self {
		Self::ok(String::new())
	}

	/// Create a response with given body and 200 OK status code.
	pub fn ok<T: Into<String>>(response: T) -> Self {
		Response {
			code: StatusCode::OK,
			content_type: HeaderValue::from_static("application/json; charset=utf-8"),
			content: response.into(),
		}
	}

	/// Create a response for plaintext internal error.
	pub fn internal_error<T: Into<String>>(msg: T) -> Self {
		Response {
			code: StatusCode::INTERNAL_SERVER_ERROR,
			content_type: plain_text(),
			content: format!("Internal Server Error: {}", msg.into()),
		}
	}

	/// Create a json response for service unavailable.
	pub fn service_unavailable<T: Into<String>>(msg: T) -> Self {
		Response {
			code: StatusCode::SERVICE_UNAVAILABLE,
			content_type: HeaderValue::from_static("application/json; charset=utf-8"),
			content: msg.into(),
		}
	}

	/// Create a response for not allowed hosts.
	pub fn host_not_allowed() -> Self {
		Response {
			code: StatusCode::FORBIDDEN,
			content_type: plain_text(),
			content: "Provided Host header is not whitelisted.\n".to_owned(),
		}
	}

	/// Create a response for unsupported content type.
	pub fn unsupported_content_type() -> Self {
		Response {
			code: StatusCode::UNSUPPORTED_MEDIA_TYPE,
			content_type: plain_text(),
			content: "Supplied content type is not allowed. Content-Type: application/json is required\n".to_owned(),
		}
	}

	/// Create a response for disallowed method used.
	pub fn method_not_allowed() -> Self {
		Response {
			code: StatusCode::METHOD_NOT_ALLOWED,
			content_type: plain_text(),
			content: "Used HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned(),
		}
	}

	/// CORS invalid
	pub fn invalid_allow_origin() -> Self {
		Response {
			code: StatusCode::FORBIDDEN,
			content_type: plain_text(),
			content: "Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n".to_owned(),
		}
	}

	/// CORS header invalid
	pub fn invalid_allow_headers() -> Self {
		Response {
			code: StatusCode::FORBIDDEN,
			content_type: plain_text(),
			content: "Requested headers are not allowed for CORS. CORS headers would not be sent and any side-effects were cancelled as well.\n".to_owned(),
		}
	}

	/// Create a response for bad request
	pub fn bad_request<S: Into<String>>(msg: S) -> Self {
		Response {
			code: StatusCode::BAD_REQUEST,
			content_type: plain_text(),
			content: msg.into(),
		}
	}

	/// Create a response for too large (413)
	pub fn too_large<S: Into<String>>(msg: S) -> Self {
		Response {
			code: StatusCode::PAYLOAD_TOO_LARGE,
			content_type: plain_text(),
			content: msg.into(),
		}
	}

	/// Create a 500 response when server is closing.
	pub(crate) fn closing() -> Self {
		Response {
			code: StatusCode::SERVICE_UNAVAILABLE,
			content_type: plain_text(),
			content: "Server is closing.".into(),
		}
	}
}

fn plain_text() -> HeaderValue {
	HeaderValue::from_static("text/plain; charset=utf-8")
}

// TODO: Consider switching to a `TryFrom` conversion once it stabilizes.
impl From<Response> for hyper::Response<Body> {
	/// Converts from a jsonrpc `Response` to a `hyper::Response`
	///
	/// ## Panics
	///
	/// Panics if the response cannot be converted due to failure to parse
	/// body content.
	///
	fn from(res: Response) -> hyper::Response<Body> {
		hyper::Response::builder()
			.status(res.code)
			.header("content-type", res.content_type)
			.body(res.content.into())
			// Parsing `StatusCode` and `HeaderValue` is infalliable but
			// parsing body content is not.
			.expect("Unable to parse response body for type conversion")
	}
}
