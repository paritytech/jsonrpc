extern crate tokio_core;

use std::io;
use stream_codec::tokio_core::io::{Codec, EasyBuf};

pub struct StreamCodec;

impl Codec for StreamCodec {
	type In = String;
	type Out = String;

	fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
		let mut depth = 0;
		let mut in_str = false;
		let mut is_escaped = false;
		let mut start_idx = 0;

		for idx in 0..buf.as_ref().len() {
			let byte = buf.as_ref()[idx];

			if (byte == b'{' || byte == b'[') && !in_str {
				if depth == 0 {
					start_idx = idx;
				}
				depth += 1;
			}
			else if (byte == b'}' || byte == b']') && !in_str {
				depth -= 1;
			}
			else if byte == b'"' && !is_escaped {
				in_str = !in_str;
			}
			else if byte == b'\\' && is_escaped && !in_str {
				is_escaped = !is_escaped;
			}

			if depth == 0 && idx != start_idx {
				let bts = buf.drain_to(idx + 1);
				match String::from_utf8(bts.as_ref().to_vec()) {
					Ok(val) => { return Ok(Some(val)) },
					Err(_) => { return Ok(None); } // skip non-utf requests (TODO: log error?)
				};
			}
		}

		Ok(None)	
	}

	fn encode(&mut self, msg: String, buf: &mut Vec<u8>) -> io::Result<()> {
		buf.extend_from_slice(msg.as_bytes());
		buf.push(b'\n'); // for #4750
		Ok(())
	}
}

#[cfg(test)]
mod tests {

	use super::StreamCodec;
	use super::tokio_core::io::{Codec, EasyBuf};

	#[test]
	fn simple_encode() {
		let mut buf = EasyBuf::new();
		// TODO: maybe ignore new lines here also if the output is enveloped with those
		buf.get_mut().extend_from_slice(b"{ test: 1 }{ test: 2 }{ test: 3 }");

		let mut codec = StreamCodec;

		let request = codec.decode(&mut buf)
			.expect("There should be no error in simple test")
			.expect("There should be at least one request in simple test");

		assert_eq!(request, "{ test: 1 }");
	}

	#[test]
	fn fragmented_encode() {
		let mut buf = EasyBuf::new();	
		// TODO: maybe ignore new lines here also if the output is enveloped with those
		buf.get_mut().extend_from_slice(b"{ test: 1 }{ test: 2 }{ tes");

		let mut codec = StreamCodec;
		let request = codec.decode(&mut buf)
			.expect("There should be no error in first fragmented test")
			.expect("There should be at least one request in first fragmented test");
		assert_eq!(request, "{ test: 1 }");
		codec.decode(&mut buf)
			.expect("There should be no error in second fragmented test")
			.expect("There should be at least one request in second fragmented test");
		assert_eq!(String::from_utf8(buf.as_ref().to_vec()).unwrap(), "{ tes");

		buf.get_mut().extend_from_slice(b"t: 3 }");
		let request = codec.decode(&mut buf)
			.expect("There should be no error in third fragmented test")
			.expect("There should be at least one request in third fragmented test");
		assert_eq!(request, "{ test: 3 }");
	}
}
