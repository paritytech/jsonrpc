use hyper::header::AccessControlAllowOrigin;
use hyper::server::Request;

pub fn read_origin(req: &Request) -> Option<String> {
	match req.headers().get_raw("origin") {
		Some(ref v) if v.len() == 1 => {
			String::from_utf8(&v[0]).ok()
		},
		_ => None
	}
}

pub fn get_cors_header(allowed: &[AccessControlAllowOrigin], origin: &Option<String>) -> Option<AccessControlAllowOrigin> {

	if allowed.is_empty() {
		return None;
	}

	if let Some(ref origin) = *origin {
		for cors in allowed  {
			match *cors {
				AccessControlAllowOrigin::Any => return Some(AccessControlAllowOrigin::Value(origin.clone())),
				AccessControlAllowOrigin::Null => return Some(AccessControlAllowOrigin::Null),
				AccessControlAllowOrigin::Value(ref val) if val == origin => return Some(AccessControlAllowOrigin::Value(origin.clone())),
				_ => {}
			}
		}
	}

	Some(AccessControlAllowOrigin::Null)
}


#[cfg(test)]
mod tests {
	use super::*;
	use hyper::header::AccessControlAllowOrigin;

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(&Vec::new(), &origin);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_empty_origin() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(
			&vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())],
			&origin
		);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Null));
	}

	#[test]
	fn should_return_none_for_not_matching_origin() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(
			&vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())],
			&origin
		);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Null));
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(&vec![AccessControlAllowOrigin::Any], &origin);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}

	#[test]
	fn should_return_specific_origin_if_there_is_a_match() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(
			&vec![AccessControlAllowOrigin::Value("http://ethereum.org".into()), AccessControlAllowOrigin::Value("http://ethcore.io".into())],
			&origin
		);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}
}
