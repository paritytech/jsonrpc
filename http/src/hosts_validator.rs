use hyper::{server, header};
use hyper::net::HttpStream;

/// Returns `true` when `Host` header in provided `Request` is whitelisted in `allowed_hosts`.
pub fn is_host_header_valid(request: &server::Request<HttpStream>, allowed_hosts: &[String]) -> bool {
	let host = request.headers().get::<header::Host>();
	is_host_valid(host, allowed_hosts)
}

fn is_host_valid(host: Option<&header::Host>, allowed_hosts: &[String]) -> bool {
	if let Some(ref host) = host {
		let hostname = match host.port {
			Some(ref port) => format!("{}:{}", host.hostname, port),
			None => host.hostname.clone(),
		};

		for h in allowed_hosts {
			if h == &hostname {
				return true
			}
		}
	}
	false
}

#[test]
fn should_reject_when_there_is_no_header() {
	let valid = is_host_valid(None, &[]);
	assert_eq!(valid, false);
}

#[test]
fn should_reject_if_header_not_on_the_list() {
	let valid = is_host_valid(Some(&header::Host { hostname: "ethcore.io".into(), port: None }), &[]);
	assert_eq!(valid, false);
}

#[test]
fn should_accept_if_on_the_list() {
	let valid = is_host_valid(
		Some(&header::Host { hostname: "ethcore.io".into(), port: None }),
		&["ethcore.io".into()]
	);
	assert_eq!(valid, true);
}

#[test]
fn should_accept_if_on_the_list_with_port() {
	let valid = is_host_valid(
		Some(&header::Host { hostname: "ethcore.io".into(), port: Some(443) }),
		&["ethcore.io:443".into()]
	);
	assert_eq!(valid, true);
}

