//! Convenient Response utils used internally

use tokio_minihttp::Response;
use cors;

pub fn method_not_allowed() -> Response {
	let mut response = Response::new();
	response
		.status_code(405, "Method Not Allowed")
		.header("Content-Type", "text/plain")
		.body("Used HTTP Method is not allowed. POST or OPTIONS is required");
	response
}

pub fn invalid_host() -> Response {
	let mut response = Response::new();
	response
		.status_code(403, "Forbidden")
		.header("Content-Type", "text/plain")
		.body("Provided Host header is not whitelisted.");
	response
}

pub fn invalid_content_type() -> Response {
	let mut response = Response::new();
	response
		.status_code(415, "Unsupported Media Type")
		.header("Content-Type", "text/plain")
		.body("Supplied content type is not allowed. Content-Type: application/json is required");
	response
}

pub fn new(body: &str, cors: Option<cors::AccessControlAllowOrigin>) -> Response {
	let mut response = Response::new();
	response
		.header("Content-Type", "application/json")
		.body(body);

	if let Some(cors) = cors {
		response
			.header("Access-Control-Allow-Methods", "OPTIONS, POST")
			.header("Access-Control-Allow-Headers", "Origin, Content-Type, Accept")
			.header("Access-Control-Allow-Origin", match cors {
				cors::AccessControlAllowOrigin::Null => "null",
				cors::AccessControlAllowOrigin::Any => "*",
				cors::AccessControlAllowOrigin::Value(ref val) => val,
			})
			.header("Vary", "Origin");
	}
	response
}
