//! CORS related utilities.

use hyper::header::AccessControlAllowOrigin;
use hyper::server::Request;
use hyper::net::HttpStream;

/// Reads Origin header from the request.
pub fn read_origin(req: &Request<HttpStream>) -> Option<String> {
	match req.headers().get_raw("origin") {
		Some(ref v) if v.len() == 1 => {
			String::from_utf8(v[0].to_vec()).ok()
		},
		_ => None
	}
}

/// Returns correct CORS header (if any) given list of allowed origins and current origin.
pub fn get_cors_header(allowed: &Option<Vec<AccessControlAllowOrigin>>, origin: &Option<String>) -> Option<AccessControlAllowOrigin> {

	if allowed.is_none() {
		return None;
	}
	let allowed = allowed.as_ref().unwrap();

	match *origin {
		Some(ref origin) => {
			allowed.iter().find(|cors| {
				match **cors {
					AccessControlAllowOrigin::Any => true,
					AccessControlAllowOrigin::Value(ref val) if val == origin => true,
					_ => false
				}
			}).map(|cors| {
				match *cors {
					AccessControlAllowOrigin::Any => AccessControlAllowOrigin::Value(origin.clone()),
					ref cors => cors.clone(),
				}
			})
		},
		None => {
			allowed.iter().find(|cors| **cors == AccessControlAllowOrigin::Null).cloned()
		},
	}
}


#[cfg(test)]
mod tests {
	use super::*;
	use hyper::header::AccessControlAllowOrigin;

	#[test]
	fn should_return_none_when_there_are_no_cors_domains() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(&None, &origin);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_empty_origin() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
			&origin
		);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(&Some(Vec::new()), &origin);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_not_matching_origin() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
			&origin
		);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(&Some(vec![AccessControlAllowOrigin::Any]), &origin);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}

	#[test]
	fn should_return_null_only_if_null_is_set_and_origin_is_not_defined() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(
			&Some(vec![AccessControlAllowOrigin::Null]),
			&origin
		);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Null));
	}

	#[test]
	fn should_return_specific_origin_if_there_is_a_match() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into()), AccessControlAllowOrigin::Value("http://ethcore.io".into())]),
			&origin
			);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}
}
