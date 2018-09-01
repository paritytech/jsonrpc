use hyper::{header, server};

use server_utils::{cors, hosts};
pub use server_utils::cors::{AllowOrigin, AllowHeaders, AccessControlAllowHeaders};

/// Extracts string value of a single header in request.
fn read_header<'a>(req: &'a server::Request, header: &str) -> Option<&'a str> {
	match req.headers().get_raw(header) {
		Some(ref v) if v.len() == 1 => {
			::std::str::from_utf8(&v[0]).ok()
		},
		_ => None
	}
}

/// Returns `true` if Host header in request matches a list of allowed hosts.
pub fn is_host_allowed(
	request: &server::Request,
	allowed_hosts: &Option<Vec<hosts::Host>>,
) -> bool {
	hosts::is_host_valid(read_header(request, "host"), allowed_hosts)
}

/// Returns a CORS header that should be returned with that request.
pub fn cors_allow_origin(
	request: &server::Request,
	cors_domains: &Option<Vec<cors::AccessControlAllowOrigin>>
) -> AllowOrigin<header::AccessControlAllowOrigin> {
	cors::get_cors_allow_origin(read_header(request, "origin"), read_header(request, "host"), cors_domains).map(|origin| {
		use self::cors::AccessControlAllowOrigin::*;
		match origin {
			Value(val) => header::AccessControlAllowOrigin::Value((*val).to_owned()),
			Null => header::AccessControlAllowOrigin::Null,
			Any => header::AccessControlAllowOrigin::Any,
		}
	})
}

/// Returns the CORS header that should be returned with that request.
pub fn cors_allow_headers(
	request: &server::Request,
	cors_allow_headers: &cors::AccessControlAllowHeaders
) -> AllowHeaders<header::AccessControlAllowHeaders> {
	cors::get_cors_allow_headers(request.headers(), cors_allow_headers)
}
