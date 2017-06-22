//! Convenient Response utils used internally

use tokio_minihttp::Response;
use jsonrpc_server_utils::cors;

const SERVER: &'static str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub fn options(cors: Option<cors::AccessControlAllowOrigin>) -> Response {
	let mut response = new("", cors);
	response
		.header("Allow", "OPTIONS, POST")
		.header("Accept", "application/json");
	response
}

pub fn method_not_allowed() -> Response {
	let mut response = Response::new();
	response
		.status_code(405, "Method Not Allowed")
		.server(SERVER)
		.header("Content-Type", "text/plain")
		.body("Used HTTP Method is not allowed. POST or OPTIONS is required.\n");
	response
}

pub fn invalid_host() -> Response {
	let mut response = Response::new();
	response
		.status_code(403, "Forbidden")
		.server(SERVER)
		.header("Content-Type", "text/plain")
		.body("Provided Host header is not whitelisted.\n");
	response
}

pub fn internal_error() -> Response {
	let mut response = Response::new();
	response
		.status_code(500, "Internal Error")
		.server(SERVER)
		.header("Content-Type", "text/plain")
		.body("Interal Server Error has occured.");
	response
}

pub fn invalid_cors() -> Response {
	let mut response = Response::new();
	response
		.status_code(403, "Forbidden")
		.server(SERVER)
		.header("Content-Type", "text/plain")
		.body("Origin of the request is not whitelisted. CORS headers would not be sent and any side-effects were cancelled as well.\n");
	response
}

pub fn invalid_content_type() -> Response {
	let mut response = Response::new();
	response
		.status_code(415, "Unsupported Media Type")
		.server(SERVER)
		.header("Content-Type", "text/plain")
		.body("Supplied content type is not allowed. Content-Type: application/json is required.\n");
	response
}

pub fn new(body: &str, cors: Option<cors::AccessControlAllowOrigin>) -> Response {
	let mut response = Response::new();
	response
		.header("Content-Type", "application/json")
		.server(SERVER)
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
