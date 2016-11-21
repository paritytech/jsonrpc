// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! Request boundary validator

use std::mem;

pub fn extract_requests(buf: &[u8]) -> (Vec<String>, usize) {
	let utf8 = match String::from_utf8(buf.to_vec()) {
		Ok(val) => val,
		Err(_) => { return (Vec::new(), 0) }
	};

	let mut str_buf = String::new();
	let mut depth = 0;
	let mut res = Vec::new();
	let mut last_req = 0;
	let mut in_str = false;
	let mut is_escaped = false;
	let mut ever_started = false;

	for (idx, char) in utf8.char_indices() {
		str_buf.push(char);

		if (char == '{' || char == '[') && !in_str {
			if depth == 0 {
				ever_started = true;
			}
			depth += 1;
		}
		else if (char == '}' || char == ']') && !in_str {
			depth -= 1;
		}
		else if char == '"' && !is_escaped {
			in_str = !in_str;
		}
		else if char == '\\' && is_escaped && !in_str {
			is_escaped = !is_escaped;
		}

		if depth == 0 && str_buf.len() > 0 {
			if ever_started {
				res.push(mem::replace(&mut str_buf, String::new()));
			}
			ever_started = false;
			last_req = idx;
		}
	}

	(res, last_req)
}

#[test]
fn can_extract_request() {
	let buf = b"{ \"val\" : 1 } ffuuu";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "{ \"val\" : 1 }");
}

#[test]
fn can_extract_requests() {
	let buf = b"{ \"val\" : 1 }{ \"val2\" : 2 }";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "{ \"val\" : 1 }");
	assert_eq!(res.0[1], "{ \"val2\" : 2 }");
}

#[test]
fn can_extract_requests_with_slash() {
	let buf = b"{ \"va\\l\" : 1 }{ \"va\\l2\" : 2 }";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "{ \"va\\l\" : 1 }");
	assert_eq!(res.0[1], "{ \"va\\l2\" : 2 }");
}

#[test]
fn can_extract_requests_with_brackets() {
	let buf = b"[{ \"val_s1\" : 10 }{ \"val_s2\" : 20 }]";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "[{ \"val_s1\" : 10 }{ \"val_s2\" : 20 }]");
}

#[test]
fn can_extract_requests_with_braces() {
	let buf = b"{ \"va{l\" : 1 }{ \"va}l2\" : 2 }";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "{ \"va{l\" : 1 }");
	assert_eq!(res.0[1], "{ \"va}l2\" : 2 }");
}

#[test]
fn can_extract_vitro1() {
	let buf = b"[{\"jsonrpc\":\"2.0\",\"id\":\"3a3472c0-c7be-4070-a05c-8042c0a94892\",\"method\":\"eth_accounts\",\"params\":[]}][{\"jsonrpc\":\"2.0\",\"id\":\"a7329aff-888e-4aa7-a925-651c9545f356\",\"method\":\"net_peerCount\",\"params\":[]}]";
	let res = extract_requests(buf);
	assert_eq!(res.0[0], "[{\"jsonrpc\":\"2.0\",\"id\":\"3a3472c0-c7be-4070-a05c-8042c0a94892\",\"method\":\"eth_accounts\",\"params\":[]}]");
	assert_eq!(res.0[1], "[{\"jsonrpc\":\"2.0\",\"id\":\"a7329aff-888e-4aa7-a925-651c9545f356\",\"method\":\"net_peerCount\",\"params\":[]}]");
}
