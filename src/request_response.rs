use hyper::status::StatusCode;
use hyper::header::ContentType;

pub struct Request {
	pub content: String,
	pub origin: Option<String>,
}

impl Request {
	pub fn empty() -> Self {
		Request {
			content: String::new(),
			origin: None,
		}
	}
}

pub struct Response {
	pub code: StatusCode,
	pub content_type: ContentType,
	pub content: String,
	pub write_pos: usize,
}

impl Response {
	pub fn empty() -> Self {
		Self::ok(String::new())
	}

	pub fn ok(response: String) -> Self {
		Response {
			code: StatusCode::Ok,
			content_type: ContentType::json(),
			content: response,
			write_pos: 0,
		}
	}

	pub fn unsupported_content_type() -> Self {
		Response {
			code: StatusCode::UnsupportedMediaType,
			content_type: ContentType::html(),
			content: "Supplied content type is not allowed. Content-Type: application/json is required\n".to_owned(),
			write_pos: 0,
		}
	}

	pub fn method_not_allowed() -> Self {
		Response {
			code: StatusCode::MethodNotAllowed,
			content_type: ContentType::html(),
			content: "Used HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned(),
			write_pos: 0,
		}
	}
}

