//! CORS handling utility functions

use std::{fmt, ops};
use std::ascii::AsciiExt;
use hosts::Host;

/// Origin Protocol
#[derive(Clone, Hash, Debug, PartialEq, Eq)]
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
	host: Host,
	as_string: String,
}

impl<T: AsRef<str>> From<T> for Origin {
	fn from(string: T) -> Self {
		Origin::parse(string.as_ref())
	}
}

impl Origin {
	fn with_host(protocol: OriginProtocol, host: Host) -> Self {
		let string = Self::to_string(&protocol, &host);

		Origin {
			protocol: protocol,
			host: host,
			as_string: string,
		}
	}

	/// Creates new origin given protocol, hostname and port parts.
	/// Pre-processes input data if necessary.
	pub fn new(protocol: OriginProtocol, host: &str, port: Option<u16>) -> Self {
		Self::with_host(protocol, Host::new(host, port))
	}

	/// Attempts to parse given string as a `Origin`.
	/// NOTE: This method always succeeds and falls back to sensible defaults.
	pub fn parse(data: &str) -> Self {
		let mut it = data.split("://");
		let proto = it.next().expect("split always returns non-empty iterator.");
		let hostname = it.next();

		let (proto, hostname) = match hostname {
			None => (None, proto),
			Some(hostname) => (Some(proto), hostname),
		};

		let proto = proto.map(str::to_lowercase);
		let hostname = Host::parse(hostname);

		let protocol = match proto {
			None => OriginProtocol::Http,
			Some(ref p) if p == "http" => OriginProtocol::Http,
			Some(ref p) if p == "https" => OriginProtocol::Https,
			Some(other) => OriginProtocol::Custom(other),
		};

		Origin::with_host(protocol, hostname)
	}

	fn to_string(protocol: &OriginProtocol, host: &Host) -> String {
		format!(
			"{}://{}",
			match *protocol {
				OriginProtocol::Http => "http",
				OriginProtocol::Https => "https",
				OriginProtocol::Custom(ref protocol) => protocol,
			},
			&**host,
		)
	}
}

impl ops::Deref for Origin {
	type Target = str;
	fn deref(&self) -> &Self::Target {
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

impl fmt::Display for AccessControlAllowOrigin {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", match *self {
			AccessControlAllowOrigin::Any => "*",
			AccessControlAllowOrigin::Null => "null",
			AccessControlAllowOrigin::Value(ref val) => val,
		})
	}
}

impl<T: Into<String>> From<T> for AccessControlAllowOrigin {
	fn from(s: T) -> AccessControlAllowOrigin {
		match s.into().as_str() {
			"all" | "*" | "any" => AccessControlAllowOrigin::Any,
			"null" => AccessControlAllowOrigin::Null,
			origin => AccessControlAllowOrigin::Value(origin.into()),
		}
	}
}

/// CORS Header Result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsHeader<T = AccessControlAllowOrigin> {
	/// CORS header was not required. Origin is not present in the request.
	NotRequired,
	/// CORS header is not returned, Origin is not allowed to access the resource.
	Invalid,
	/// CORS header to include in the response. Origin is allowed to access the resource.
	Ok(T),
}

impl<T> CorsHeader<T> {
	/// Maps `Ok` variant of `CorsHeader`.
	pub fn map<F, O>(self, f: F) -> CorsHeader<O> where
		F: FnOnce(T) -> O,
	{
		use self::CorsHeader::*;

		match self {
			NotRequired => NotRequired,
			Invalid => Invalid,
			Ok(val) => Ok(f(val)),
		}
	}
}

impl<T> Into<Option<T>> for CorsHeader<T> {
	fn into(self) -> Option<T> {
		use self::CorsHeader::*;

		match self {
			NotRequired | Invalid => None,
			Ok(header) => Some(header),
		}
	}
}

/// Returns correct CORS header (if any) given list of allowed origins and current origin.
pub fn get_cors_header(origin: Option<&str>, allowed: &Option<Vec<AccessControlAllowOrigin>>) -> CorsHeader {
	match origin {
		None => CorsHeader::NotRequired,
		Some(ref origin) => match allowed.as_ref() {
			None => CorsHeader::Ok(AccessControlAllowOrigin::Value(Origin::parse(origin))),
			Some(ref allowed) if *origin == "null" => {
				allowed.iter().find(|cors| **cors == AccessControlAllowOrigin::Null).cloned()
					.map(CorsHeader::Ok)
					.unwrap_or(CorsHeader::Invalid)
			},
			Some(ref allowed) => {
				allowed.iter().find(|cors| {
					match **cors {
						AccessControlAllowOrigin::Any => true,
						AccessControlAllowOrigin::Value(ref val) if val.eq_ignore_ascii_case(origin) => true,
						_ => false
					}
				}).map(|cors| {
					match *cors {
						AccessControlAllowOrigin::Any => AccessControlAllowOrigin::Value(Origin::parse(origin)),
						ref cors => cors.clone(),
					}
				}).map(CorsHeader::Ok).unwrap_or(CorsHeader::Invalid)
			},
		},
	}
}


#[cfg(test)]
mod tests {
	use super::{get_cors_header, CorsHeader, AccessControlAllowOrigin, Origin, OriginProtocol};

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
	fn should_return_none_when_there_are_no_cors_domains_and_no_origin() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(origin, &None);

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_domain_when_all_are_allowed() {
		// given
		let origin = Some("parity.io");

		// when
		let res = get_cors_header(origin, &None);

		// then
		assert_eq!(res, CorsHeader::Ok("parity.io".into()));
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
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;

		// when
		let res = get_cors_header(origin, &Some(Vec::new()));

		// then
		assert_eq!(res, CorsHeader::NotRequired);
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
		assert_eq!(res, CorsHeader::Invalid);
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://ethcore.io".into());

		// when
		let res = get_cors_header(origin, &Some(vec![AccessControlAllowOrigin::Any]));

		// then
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
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
		assert_eq!(res, CorsHeader::NotRequired);
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
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Null));
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
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Value("http://ethcore.io".into())));
	}
}
