//! CORS handling utility functions

use std::{fmt, ops};
use hosts::{Host, Port};
use matcher::{Matcher, Pattern};

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
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Origin {
	protocol: OriginProtocol,
	host: Host,
	as_string: String,
	matcher: Matcher,
}

impl<T: AsRef<str>> From<T> for Origin {
	fn from(string: T) -> Self {
		Origin::parse(string.as_ref())
	}
}

impl Origin {
	fn with_host(protocol: OriginProtocol, host: Host) -> Self {
		let string = Self::to_string(&protocol, &host);
		let matcher = Matcher::new(&string);

		Origin {
			protocol: protocol,
			host: host,
			as_string: string,
			matcher: matcher,
		}
	}

	/// Creates new origin given protocol, hostname and port parts.
	/// Pre-processes input data if necessary.
	pub fn new<T: Into<Port>>(protocol: OriginProtocol, host: &str, port: T) -> Self {
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

impl Pattern for Origin {
	fn matches<T: AsRef<str>>(&self, other: T) -> bool {
		self.matcher.matches(other)
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
pub fn get_cors_header(origin: Option<&str>, host: Option<&str>, allowed: &Option<Vec<AccessControlAllowOrigin>>) -> CorsHeader {
	match origin {
		None => CorsHeader::NotRequired,
		Some(ref origin) => {
			if let Some(host) = host {
				// Request initiated from the same server.
				if origin.ends_with(host) {
					// Additional check
					let origin = Origin::parse(origin);
					if &*origin.host == host {
						return CorsHeader::NotRequired;
					}
				}
			}

			match allowed.as_ref() {
				None if *origin == "null" => CorsHeader::Ok(AccessControlAllowOrigin::Null),
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
							AccessControlAllowOrigin::Value(ref val) if val.matches(origin) => true,
							_ => false
						}
					})
					.map(|_| AccessControlAllowOrigin::Value(Origin::parse(origin)))
					.map(CorsHeader::Ok).unwrap_or(CorsHeader::Invalid)
				},
			}
		},
	}
}


#[cfg(test)]
mod tests {
	use hosts::Host;
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
	fn should_not_allow_partially_matching_origin() {
		// given
		let origin1 = Origin::parse("http://subdomain.somedomain.io");
		let origin2 = Origin::parse("http://somedomain.io:8080");
		let host = Host::parse("http://somedomain.io");

		let origin1 = Some(&*origin1);
		let origin2 = Some(&*origin2);
		let host = Some(&*host);

		// when
		let res1 = get_cors_header(origin1, host, &Some(vec![]));
		let res2 = get_cors_header(origin2, host, &Some(vec![]));

		// then
		assert_eq!(res1, CorsHeader::Invalid);
		assert_eq!(res2, CorsHeader::Invalid);
	}

	#[test]
	fn should_allow_origins_that_matches_hosts() {
		// given
		let origin = Origin::parse("http://127.0.0.1:8080");
		let host = Host::parse("http://127.0.0.1:8080");

		let origin = Some(&*origin);
		let host = Some(&*host);

		// when
		let res = get_cors_header(origin, host, &None);

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_none_when_there_are_no_cors_domains_and_no_origin() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_header(origin, host, &None);

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_domain_when_all_are_allowed() {
		// given
		let origin = Some("parity.io");
		let host = None;

		// when
		let res = get_cors_header(origin, host, &None);

		// then
		assert_eq!(res, CorsHeader::Ok("parity.io".into()));
	}

	#[test]
	fn should_return_none_for_empty_origin() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_header(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_header(origin, host, &Some(Vec::new()));

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_none_for_not_matching_origin() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_header(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, CorsHeader::Invalid);
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_header(origin, host, &Some(vec![AccessControlAllowOrigin::Any]));

		// then
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
	}

	#[test]
	fn should_return_none_if_origin_is_not_defined() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_header(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Null]),
		);

		// then
		assert_eq!(res, CorsHeader::NotRequired);
	}

	#[test]
	fn should_return_null_if_origin_is_null() {
		// given
		let origin = Some("null".into());
		let host = None;

		// when
		let res = get_cors_header(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Null]),
		);

		// then
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Null));
	}

	#[test]
	fn should_return_specific_origin_if_there_is_a_match() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_header(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into()), AccessControlAllowOrigin::Value("http://parity.io".into())]),
		);

		// then
		assert_eq!(res, CorsHeader::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
	}

	#[test]
	fn should_support_wildcards() {
		// given
		let origin1 = Some("http://parity.io".into());
		let origin2 = Some("http://parity.iot".into());
		let origin3 = Some("chrome-extension://test".into());
		let host = None;
		let allowed = Some(vec![
		   AccessControlAllowOrigin::Value("http://*.io".into()),
		   AccessControlAllowOrigin::Value("chrome-extension://*".into())
		]);

		// when
		let res1 = get_cors_header(origin1, host, &allowed);
		let res2 = get_cors_header(origin2, host, &allowed);
		let res3 = get_cors_header(origin3, host, &allowed);

		// then
		assert_eq!(res1, CorsHeader::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
		assert_eq!(res2, CorsHeader::Invalid);
		assert_eq!(res3, CorsHeader::Ok(AccessControlAllowOrigin::Value("chrome-extension://test".into())));
	}
}
