//! CORS handling utility functions

use std::ascii::AsciiExt;

/// Origin Protocol
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginProtocol {
	/// Http protocol
	Http,
	/// Https protocol
	Https,
	/// Custom protocol
	Custom(String),
}

/// Request Origin
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Origin {
	protocol: OriginProtocol,
	hostname: String,
	port: Option<u16>,
	as_string: String,
}

impl From<&'static str> for Origin {
	fn from(string: &'static str) -> Self {
		Origin::parse(string)
	}
}

impl Origin {
	/// Creates new origin given protocol, hostname and port parts.
	/// Pre-processes input data if necessary.
	pub fn new(protocol: OriginProtocol, hostname: &str, port: Option<u16>) -> Self {
		let string = Self::to_string(&protocol, hostname, port);
		Origin {
			protocol: protocol,
			hostname: hostname.into(),
			port: port,
			as_string: string,
		}
	}

	/// Attempts to parse given string as a `Origin`.
	/// NOTE: This method always succeeds and falls back to sensible defaults.
	pub fn parse(data: &str) -> Self {
		let mut it = data.split("://");
		let mut proto = it.next();
		let mut hostname = it.next();

		if hostname.is_none() {
			hostname = proto;
			proto = None;
		}

		let proto = proto.map(|s| s.to_lowercase());
		let hostname = hostname.map(|s| {
			let mut it = s.split('/');
			it.next().unwrap().to_lowercase()
		}).unwrap();

		let protocol = match proto {
			None => OriginProtocol::Http,
			Some(ref p) if p == "http" => OriginProtocol::Http,
			Some(ref p) if p == "https" => OriginProtocol::Https,
			Some(other) => OriginProtocol::Custom(other),
		};

		let mut hostname = hostname.split(':');
		let host = hostname.next().unwrap();
		let port = hostname.next().and_then(|port| port.parse().ok());
		Origin::new(protocol, &host, port)
	}

	fn to_string(protocol: &OriginProtocol, hostname: &str, port: Option<u16>) -> String {
		format!(
			"{}://{}{}",
			match *protocol {
				OriginProtocol::Http => "http",
				OriginProtocol::Https => "https",
				OriginProtocol::Custom(ref protocol) => protocol,
			},
			hostname,
			match port {
				Some(port) => format!(":{}", port),
				None => "".into(),
			},
		)
	}
}

impl AsRef<str> for Origin {
	fn as_ref(&self) -> &str {
		&self.as_string
	}
}

/// Origins allowed to access
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessControlAllowOrigin {
	/// Specific hostname
	Value(Origin),
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
						AccessControlAllowOrigin::Value(ref val) if val.as_ref().eq_ignore_ascii_case(origin) => true,
						_ => false
					}
				}).map(|cors| {
					match *cors {
						AccessControlAllowOrigin::Any => AccessControlAllowOrigin::Value(Origin::parse(origin)),
						ref cors => cors.clone(),
					}
				})
			},
		}
	}
}


#[cfg(test)]
mod tests {
	use super::{get_cors_header, AccessControlAllowOrigin, Origin, OriginProtocol};

	#[test]
	fn should_parse_origin() {
		use self::OriginProtocol::*;

		assert_eq!(Origin::parse("http://parity.io"), Origin::new(Http, "parity.io", None));
		assert_eq!(Origin::parse("https://parity.io:8443"), Origin::new(Https, "parity.io", Some(8443)));
		assert_eq!(Origin::parse("chrome-extension://124.0.0.1"), Origin::new(Custom("chrome-extension".into()), "124.0.0.1", None));
		assert_eq!(Origin::parse("parity.io/somepath"), Origin::new(Http, "parity.io", None));
		assert_eq!(Origin::parse("127.0.0.1:8545/somepath"), Origin::new(Http, "127.0.0.1", Some(8545)));
	}

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
