//! Host header validation.

use std::ascii::AsciiExt;

// TODO [ToDr] Pre-process hosts (so that they don't contain the path)

/// Host type
pub type Host = String;

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
				allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(host) || h == host)
			}
		}
	}
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

