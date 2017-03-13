use std::io;
use uds::server_utils::tokio_core::io::{Codec, EasyBuf};

pub struct StreamCodec;

fn is_whitespace(byte: u8) -> bool {
	match byte {
		0x0D | 0x0A | 0x20 | 0x09 => true,
		_ => false,
	} 
}

impl Codec for StreamCodec {
	type In = String;
	type Out = String;

	fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
		let mut depth = 0;
		let mut in_str = false;
		let mut is_escaped = false;
		let mut start_idx = 0;
		let mut whitespaces = 0;

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
			else if is_whitespace(byte) {
				whitespaces += 1;
			}

			if depth == 0 && idx != start_idx && idx - start_idx + 1 > whitespaces {
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
	use uds::server_utils::tokio_core::io::{Codec, EasyBuf};

	#[test]
	fn simple_encode() {
		let mut buf = EasyBuf::new();
		buf.get_mut().extend_from_slice(b"{ test: 1 }{ test: 2 }{ test: 3 }");

		let mut codec = StreamCodec;

		let request = codec.decode(&mut buf)
			.expect("There should be no error in simple test")
			.expect("There should be at least one request in simple test");

		assert_eq!(request, "{ test: 1 }");
	}

	#[test]
	fn whitespace() {
		let mut buf = EasyBuf::new();
		buf.get_mut().extend_from_slice(b"{ test: 1 }\n\n\n\n{ test: 2 }\n\r{\n test: 3 }  ");

		let mut codec = StreamCodec;

		let request = codec.decode(&mut buf)
			.expect("There should be no error in first whitespace test")
			.expect("There should be a request in first whitespace test");

		assert_eq!(request, "{ test: 1 }");

		let request2 = codec.decode(&mut buf)
			.expect("There should be no error in first 2nd test")
			.expect("There should be aa request in 2nd whitespace test");
		// TODO: maybe actually trim it out
		assert_eq!(request2, "\n\n\n\n{ test: 2 }");

		let request3 = codec.decode(&mut buf)
			.expect("There should be no error in first 3rd test")
			.expect("There should be a request in 3rd whitespace test");
		assert_eq!(request3, "\n\r{\n test: 3 }");

		let request4 = codec.decode(&mut buf)
			.expect("There should be no error in first 4th test");
		assert!(request4.is_none(), "There should be no 4th request because it contains only whitespaces");
	}

	#[test]
	fn fragmented_encode() {
		let mut buf = EasyBuf::new();	
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
