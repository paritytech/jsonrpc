use hyper::{self, header};

use server_utils::{cors, hosts};
pub use server_utils::cors::CorsHeader;

/// Extracts string value of a single header in request.
fn read_header<'a>(req: &'a hyper::Request<hyper::Body>, header_name: &str) -> Option<&'a str> {
	req.headers().get(header_name).and_then(|v| v.to_str().ok())
}

/// Returns `true` if Host header in request matches a list of allowed hosts.
pub fn is_host_allowed(
	request: &hyper::Request<hyper::Body>,
	allowed_hosts: &Option<Vec<hosts::Host>>,
) -> bool {
	hosts::is_host_valid(read_header(request, "host"), allowed_hosts)
}

/// Returns a CORS header that should be returned with that request.
pub fn cors_header(
	request: &hyper::Request<hyper::Body>,
	cors_domains: &Option<Vec<cors::AccessControlAllowOrigin>>
) -> CorsHeader<header::HeaderValue> {
	cors::get_cors_header(read_header(request, "origin"), read_header(request, "host"), cors_domains).map(|origin| {
		use self::cors::AccessControlAllowOrigin::*;
		match origin {
			Value(ref val) => header::HeaderValue::from_str(val).unwrap_or(header::HeaderValue::from_static("null")),
			Null => header::HeaderValue::from_static("null"),
			Any => header::HeaderValue::from_static("*"),
		}
	})
}
