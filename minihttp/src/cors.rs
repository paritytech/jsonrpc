//! CORS handling utility functions

use std::ascii::AsciiExt;

/// Origins allowed to access
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessControlAllowOrigin {
	/// Specific hostname
	Value(String),
	/// null-origin (file:///, sandboxed iframe)
	Null,
	/// Any non-null origin
	Any,
}

/// Returns correct CORS header (if any) given list of allowed origins and current origin.
pub fn get_cors_header(origin: Option<&str>, allowed: &Option<Vec<AccessControlAllowOrigin>>) -> Option<AccessControlAllowOrigin> {
	match allowed.as_ref() {
		None => None,
		Some(ref allowed) => match origin {
			None => None,
			Some("null") => {
				allowed.iter().find(|cors| **cors == AccessControlAllowOrigin::Null).cloned()
			},
			Some(ref origin) => {
				allowed.iter().find(|cors| {
					match **cors {
						AccessControlAllowOrigin::Any => true,
						AccessControlAllowOrigin::Value(ref val) if val.eq_ignore_ascii_case(origin) => true,
						_ => false
					}
				}).map(|cors| {
					match *cors {
						AccessControlAllowOrigin::Any => AccessControlAllowOrigin::Value((*origin).to_owned()),
						ref cors => cors.clone(),
					}
				})
			},
		}
	}
}


#[cfg(test)]
mod tests {
	use super::{get_cors_header, AccessControlAllowOrigin};

	#[test]
	fn should_return_none_when_there_are_no_cors_domains() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(origin, &None);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_empty_origin() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(
			origin,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(origin, &Some(Vec::new()));

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_none_for_not_matching_origin() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(
			origin,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(origin, &Some(vec![AccessControlAllowOrigin::Any]));

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}

	#[test]
	fn should_return_none_if_origin_is_not_defined() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(
			origin,
			&Some(vec![AccessControlAllowOrigin::Null]),
		);

		// then
		assert_eq!(res, None);
	}

	#[test]
	fn should_return_null_if_origin_is_null() {
		// given
		let origin = Some("null".into());

		// when
		let res = get_cors_header(
			origin,
			&Some(vec![AccessControlAllowOrigin::Null]),
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
			origin,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into()), AccessControlAllowOrigin::Value("http://ethcore.io".into())]),
		);

		// then
		assert_eq!(res, Some(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}
}
