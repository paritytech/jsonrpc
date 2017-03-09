use std::{io, str};
use server_utils::tokio_core::io::{Codec, EasyBuf};

pub struct LineCodec;

impl Codec for LineCodec {
	type In = String;
	type Out = String;

	fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
		if let Some(i) = buf.as_slice().iter().position(|&b| b == b'\n') {
			let line = buf.drain_to(i);
			buf.drain_to(1);

			match str::from_utf8(&line.as_ref()) {
				Ok(s) => Ok(Some(s.to_string())),
				Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid UTF-8")),
			}
		} else {
			Ok(None)
		}
	}

	fn encode(&mut self, msg: String, buf: &mut Vec<u8>) -> io::Result<()> {
		buf.extend_from_slice(msg.as_bytes());
		buf.push(b'\n');
		Ok(())
	}
}

#[cfg(test)]
mod tests {

	use super::LineCodec;
	use server_utils::tokio_core::io::{Codec, EasyBuf};

	#[test]
	fn simple_encode() {
		let mut buf = EasyBuf::new();
		buf.get_mut().extend_from_slice(b"{ test: 1 }\n{ test: 2 }\n{ test: 3 }");

		let mut codec = LineCodec;

		let request = codec.decode(&mut buf)
			.expect("There should be no error in simple test")
			.expect("There should be at least one request in simple test");
		let request2 = codec.decode(&mut buf)
			.expect("There should be no error in simple test")
			.expect("There should be at least one request in simple test");

		assert_eq!(request, "{ test: 1 }");
		assert_eq!(request2, "{ test: 2 }");
	}
}
