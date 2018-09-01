//! CORS handling utility functions
extern crate hyper;
extern crate unicase;

use std::{fmt, ops};
use hosts::{Host, Port};
use matcher::{Matcher, Pattern};
use std::ops::Deref;
pub use cors::hyper::header;
pub use cors::hyper::header::AccessControlRequestHeaders;
pub use cors::hyper::Headers;
pub use self::unicase::Ascii;

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

/// CORS Allow-Origin Header Result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowOrigin<T = AccessControlAllowOrigin> {
	/// CORS header was not required. Origin is not present in the request.
	NotRequired,
	/// CORS header is not returned, Origin is not allowed to access the resource.
	Invalid,
	/// CORS header to include in the response. Origin is allowed to access the resource.
	Ok(T),
}

/// Headers allowed to access
#[derive(Debug, Clone, PartialEq)]
pub enum AccessControlAllowHeaders<T = Vec<Ascii<String>>> {
	/// Specific headers
	Only(T),
	/// Any non-null origin
	Any,
}

/// CORS Allow-Headers Result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowHeaders<T = header::AccessControlAllowHeaders> {
	/// CORS header was not required. Request-Headers is not present in the request.
	NotRequired,
	/// CORS header is not returned, Request-Headers are not allowed to access the resource.
	Invalid,
	/// CORS header to include in the response. Origin Request-Headers are allowed to access the resource.
	Ok(T),
}

impl Into<Vec<Ascii<String>>> for AccessControlAllowHeaders {
	fn into(self) -> Vec<Ascii<String>> {
		use self::AccessControlAllowHeaders::Any;
		use self::AccessControlAllowHeaders::Only;

		match self {
			Any => vec![Ascii::new("*".to_owned())],
			Only(h) => h,
		}
	}
}

impl Into<AccessControlAllowHeaders> for Vec<Ascii<String>>  {
	fn into(self) -> AccessControlAllowHeaders {
		AccessControlAllowHeaders::Only(self)
	}
}

impl<T> AllowOrigin<T> {
	/// Maps `Ok` variant of `AllowOrigin`.
	pub fn map<F, O>(self, f: F) -> AllowOrigin<O> where
		F: FnOnce(T) -> O,
	{
		use self::AllowOrigin::*;

		match self {
			NotRequired => NotRequired,
			Invalid => Invalid,
			Ok(val) => Ok(f(val)),
		}
	}
}

impl<T> Into<Option<T>> for AllowOrigin<T> {
	fn into(self) -> Option<T> {
		use self::AllowOrigin::*;

		match self {
			NotRequired | Invalid => None,
			Ok(header) => Some(header),
		}
	}
}

/// Returns correct CORS header (if any) given list of allowed origins and current origin.
pub fn get_cors_allow_origin(origin: Option<&str>, host: Option<&str>, allowed: &Option<Vec<AccessControlAllowOrigin>>) -> AllowOrigin {
	match origin {
		None => AllowOrigin::NotRequired,
		Some(ref origin) => {
			if let Some(host) = host {
				// Request initiated from the same server.
				if origin.ends_with(host) {
					// Additional check
					let origin = Origin::parse(origin);
					if &*origin.host == host {
						return AllowOrigin::NotRequired;
					}
				}
			}

			match allowed.as_ref() {
				None if *origin == "null" => AllowOrigin::Ok(AccessControlAllowOrigin::Null),
				None => AllowOrigin::Ok(AccessControlAllowOrigin::Value(Origin::parse(origin))),
				Some(ref allowed) if *origin == "null" => {
					allowed.iter().find(|cors| **cors == AccessControlAllowOrigin::Null).cloned()
						.map(AllowOrigin::Ok)
						.unwrap_or(AllowOrigin::Invalid)
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
					.map(AllowOrigin::Ok).unwrap_or(AllowOrigin::Invalid)
				},
			}
		},
	}
}

/// Validates if the `AccessControlAllowedHeaders` in the request are allowed.
pub fn get_cors_allow_headers(request_headers: &Headers, cors_allow_headers: &AccessControlAllowHeaders) -> AllowHeaders {
	// Check if the header fields which were sent in the request are required
	if let AccessControlAllowHeaders::Only(only) = cors_allow_headers {
		let are_all_allowed = request_headers.iter()
			.all(|header| {
				only.contains(&Ascii::new(header.name().to_owned())) ||
				get_always_allowed_headers().contains(&Ascii::new(header.name()))
			});

		if !are_all_allowed {
			return AllowHeaders::Invalid;
		}
	}

	// Check if `AccessControlRequestHeaders` contains fields which were allowed
	match request_headers.get::<AccessControlRequestHeaders>() {
		None => {
			// No fields were requested, no comparison necessary
			match cors_allow_headers {
				AccessControlAllowHeaders::Any => AllowHeaders::NotRequired,

				// The fields which can be used are constrainted, but since it
				// was asked for none we don't return any.
				AccessControlAllowHeaders::Only(_) => {
					let empty = header::AccessControlAllowHeaders(vec![]);
					AllowHeaders::Ok(empty)
				},
			}
		},
		Some(requested) => {
			// "requested" contains the fields for which it is inquired to know
			// if they can be used.

			let echo = AllowHeaders::Ok(
				header::AccessControlAllowHeaders(requested.deref().clone())
			);

			match cors_allow_headers {
				AccessControlAllowHeaders::Any => {
					// Any field is allowed. Our response are the fields about which
					// the request inquired.
					echo
				},

				AccessControlAllowHeaders::Only(only) => {
					let are_all_allowed = requested.iter()
						.all(|header| {
							let name = &Ascii::new(header.as_ref());
							only.contains(header) || get_always_allowed_headers().contains(name)
						});

					if !are_all_allowed {
						return AllowHeaders::Invalid;
					}

					echo
				}
			}
		}
	}
}

/// Returns headers which are always allowed.
pub fn get_always_allowed_headers() -> Vec<Ascii<&'static str>> {
	vec![
		Ascii::new("Accept"),
		Ascii::new("Accept-Language"),
		Ascii::new("Access-Control-Allow-Origin"),
		Ascii::new("Access-Control-Request-Headers"),
		Ascii::new("Content-Language"),
		Ascii::new("Content-Type"),
		Ascii::new("Host"),
		Ascii::new("Origin"),
		Ascii::new("Content-Length"),
		Ascii::new("Connection"),
		Ascii::new("User-Agent"),
	]
}

#[cfg(test)]
mod tests {
	use hosts::Host;
	use super::{get_cors_allow_origin, AllowOrigin, AccessControlAllowOrigin, Origin, OriginProtocol};
	use super::{get_cors_allow_headers, AccessControlAllowHeaders, AccessControlRequestHeaders};
	use super::{Headers, Ascii, AllowHeaders, header};

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
		let res1 = get_cors_allow_origin(origin1, host, &Some(vec![]));
		let res2 = get_cors_allow_origin(origin2, host, &Some(vec![]));

		// then
		assert_eq!(res1, AllowOrigin::Invalid);
		assert_eq!(res2, AllowOrigin::Invalid);
	}

	#[test]
	fn should_allow_origins_that_matches_hosts() {
		// given
		let origin = Origin::parse("http://127.0.0.1:8080");
		let host = Host::parse("http://127.0.0.1:8080");

		let origin = Some(&*origin);
		let host = Some(&*host);

		// when
		let res = get_cors_allow_origin(origin, host, &None);

		// then
		assert_eq!(res, AllowOrigin::NotRequired);
	}

	#[test]
	fn should_return_none_when_there_are_no_cors_domains_and_no_origin() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_allow_origin(origin, host, &None);

		// then
		assert_eq!(res, AllowOrigin::NotRequired);
	}

	#[test]
	fn should_return_domain_when_all_are_allowed() {
		// given
		let origin = Some("parity.io");
		let host = None;

		// when
		let res = get_cors_allow_origin(origin, host, &None);

		// then
		assert_eq!(res, AllowOrigin::Ok("parity.io".into()));
	}

	#[test]
	fn should_return_none_for_empty_origin() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_allow_origin(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, AllowOrigin::NotRequired);
	}

	#[test]
	fn should_return_none_for_empty_list() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_allow_origin(origin, host, &Some(Vec::new()));

		// then
		assert_eq!(res, AllowOrigin::NotRequired);
	}

	#[test]
	fn should_return_none_for_not_matching_origin() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_allow_origin(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into())]),
		);

		// then
		assert_eq!(res, AllowOrigin::Invalid);
	}

	#[test]
	fn should_return_specific_origin_if_we_allow_any() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_allow_origin(origin, host, &Some(vec![AccessControlAllowOrigin::Any]));

		// then
		assert_eq!(res, AllowOrigin::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
	}

	#[test]
	fn should_return_none_if_origin_is_not_defined() {
		// given
		let origin = None;
		let host = None;

		// when
		let res = get_cors_allow_origin(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Null]),
		);

		// then
		assert_eq!(res, AllowOrigin::NotRequired);
	}

	#[test]
	fn should_return_null_if_origin_is_null() {
		// given
		let origin = Some("null".into());
		let host = None;

		// when
		let res = get_cors_allow_origin(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Null]),
		);

		// then
		assert_eq!(res, AllowOrigin::Ok(AccessControlAllowOrigin::Null));
	}

	#[test]
	fn should_return_specific_origin_if_there_is_a_match() {
		// given
		let origin = Some("http://parity.io".into());
		let host = None;

		// when
		let res = get_cors_allow_origin(
			origin,
			host,
			&Some(vec![AccessControlAllowOrigin::Value("http://ethereum.org".into()), AccessControlAllowOrigin::Value("http://parity.io".into())]),
		);

		// then
		assert_eq!(res, AllowOrigin::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
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
		let res1 = get_cors_allow_origin(origin1, host, &allowed);
		let res2 = get_cors_allow_origin(origin2, host, &allowed);
		let res3 = get_cors_allow_origin(origin3, host, &allowed);

		// then
		assert_eq!(res1, AllowOrigin::Ok(AccessControlAllowOrigin::Value("http://parity.io".into())));
		assert_eq!(res2, AllowOrigin::Invalid);
		assert_eq!(res3, AllowOrigin::Ok(AccessControlAllowOrigin::Value("chrome-extension://test".into())));
	}

	#[test]
	fn should_return_invalid_if_header_not_allowed() {
		// given
		let cors_allow_headers = AccessControlAllowHeaders::Only(
			vec![
				Ascii::new("x-allowed".to_owned()),
			]);
		let mut request_headers = Headers::new();
		request_headers.set::<AccessControlRequestHeaders>(
			AccessControlRequestHeaders(vec![
				Ascii::new("x-not-allowed".to_owned()),
			])
		);

		// when
		let res = get_cors_allow_headers(&request_headers, &cors_allow_headers);

		// then
		assert_eq!(res, AllowHeaders::Invalid);
	}

	#[test]
	fn should_return_valid_if_header_allowed() {
		// given
		let allowed = vec![
			Ascii::new("x-allowed".to_owned()),
		];
		let cors_allow_headers = AccessControlAllowHeaders::Only(allowed.clone());
		let mut request_headers = Headers::new();
		request_headers.set::<AccessControlRequestHeaders>(
			AccessControlRequestHeaders(vec![
				Ascii::new("x-allowed".to_owned()),
			])
		);

		// when
		let res = get_cors_allow_headers(&request_headers, &cors_allow_headers);

		// then
		assert_eq!(res, AllowHeaders::Ok(header::AccessControlAllowHeaders(allowed)));
	}

	#[test]
	fn should_return_no_allowed_headers_if_none_in_request() {
		// given
		let allowed = vec![
			Ascii::new("x-allowed".to_owned()),
		];
		let cors_allow_headers = AccessControlAllowHeaders::Only(allowed.clone());
		let request_headers = Headers::new();

		// when
		let res = get_cors_allow_headers(&request_headers, &cors_allow_headers);

		// then
		assert_eq!(res, AllowHeaders::Ok(header::AccessControlAllowHeaders(vec![])));
	}

	#[test]
	fn should_return_not_required_if_any_header_allowed() {
		// given
		let cors_allow_headers = AccessControlAllowHeaders::Any;
		let request_headers = Headers::new();

		// when
		let res = get_cors_allow_headers(&request_headers, &cors_allow_headers);

		// then
		assert_eq!(res, AllowHeaders::NotRequired);
	}

}
