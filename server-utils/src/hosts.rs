//! Host header validation.

use std::ascii::AsciiExt;

/// Host type
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Host {
	hostname: String,
	port: Option<u16>,
	as_string: String,
}

impl<'a> From<&'a str> for Host {
	fn from(string: &'a str) -> Self {
		Host::parse(string)
	}
}

impl Host {
	/// Creates a new `Host` given hostname and port number.
	pub fn new(hostname: &str, port: Option<u16>) -> Self {
		let hostname = Self::pre_process(hostname);
		let string = Self::to_string(&hostname, port);

		Host {
			hostname: hostname,
			port: port,
			as_string: string,
		}
	}

	/// Attempts to parse given string as a `Host`.
	/// NOTE: This method always succeeds and falls back to sensible defaults.
	pub fn parse(hostname: &str) -> Self {
		let hostname = Self::pre_process(hostname);
		let mut hostname = hostname.split(':');
		let host = hostname.next().unwrap();
		let port = hostname.next().and_then(|port| port.parse().ok());

		Host::new(host, port)
	}

	fn pre_process(host: &str) -> String {
		// Remove possible protocol definition
		let mut it = host.split("://");
		let protocol = it.next().unwrap();
		let host = match it.next() {
			Some(data) => data,
			None => protocol,
		};

		let mut it = host.split('/');
		it.next().unwrap().to_lowercase()
	}

	fn to_string(hostname: &str, port: Option<u16>) -> String {
		format!(
			"{}{}",
			hostname,
			match port {
				Some(port) => format!(":{}", port),
				None => "".into(),
			},
		)
	}
}

impl AsRef<str> for Host {
	fn as_ref(&self) -> &str {
		&self.as_string
	}
}

/// Specifies if domains should be validated.
pub enum DomainsValidation<T> {
	/// Allow only domains on the list.
	AllowOnly(Vec<T>),
	/// Disable domains validation completely.
	Disabled,
}

impl<T> Into<Option<Vec<T>>> for DomainsValidation<T> {
	fn into(self) -> Option<Vec<T>> {
		use self::DomainsValidation::*;
		match self {
			AllowOnly(list) => Some(list),
			Disabled => None,
		}
	}
}

impl<T> From<Option<Vec<T>>> for DomainsValidation<T> {
	fn from(other: Option<Vec<T>>) -> Self {
		match other {
			Some(list) => DomainsValidation::AllowOnly(list),
			None => DomainsValidation::Disabled,
		}
	}
}

/// Returns `true` when `Host` header is whitelisted in `allowed_hosts`.
pub fn is_host_valid(host: Option<&str>, allowed_hosts: &Option<Vec<Host>>) -> bool {
	match allowed_hosts.as_ref() {
		None => true,
		Some(ref allowed_hosts) => match host {
			None => false,
			Some(ref host) => {
				allowed_hosts.iter().any(|h| {
					let h_str = h.as_ref();
					h_str.eq_ignore_ascii_case(host) || &h_str == host
				})
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{Host, is_host_valid};

	#[test]
	fn should_parse_host() {
		assert_eq!(Host::parse("http://parity.io"), Host::new("parity.io", None));
		assert_eq!(Host::parse("https://parity.io:8443"), Host::new("parity.io", Some(8443)));
		assert_eq!(Host::parse("chrome-extension://124.0.0.1"), Host::new("124.0.0.1", None));
		assert_eq!(Host::parse("parity.io/somepath"), Host::new("parity.io", None));
		assert_eq!(Host::parse("127.0.0.1:8545/somepath"), Host::new("127.0.0.1", Some(8545)));
	}

	#[test]
	fn should_reject_when_there_is_no_header() {
		let valid = is_host_valid(None, &Some(vec![]));
		assert_eq!(valid, false);
	}

	#[test]
	fn should_reject_when_validation_is_disabled() {
		let valid = is_host_valid(Some("any"), &None);
		assert_eq!(valid, true);
	}

	#[test]
	fn should_reject_if_header_not_on_the_list() {
		let valid = is_host_valid(Some("ethcore.io"), &Some(vec![]));
		assert_eq!(valid, false);
	}

	#[test]
	fn should_accept_if_on_the_list() {
		let valid = is_host_valid(
			Some("ethcore.io"),
			&Some(vec!["ethcore.io".into()]),
		);
		assert_eq!(valid, true);
	}

	#[test]
	fn should_accept_if_on_the_list_with_port() {
		let valid = is_host_valid(
			Some("ethcore.io:443"),
			&Some(vec!["ethcore.io:443".into()]),
		);
		assert_eq!(valid, true);
	}
}
