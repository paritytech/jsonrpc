use std::{io, str};
use server_utils::tokio_io::codec::{Decoder, Encoder};
use bytes::{BytesMut, BufMut};

pub struct LineCodec;

impl Decoder for LineCodec {
	type Item = String;
	type Error = io::Error;

	fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<Self::Item>> {
		if let Some(i) = buf.as_ref().iter().position(|&b| b == b'\n') {
			let line = buf.split_to(i);
			buf.split_to(1);

			match str::from_utf8(&line.as_ref()) {
				Ok(s) => Ok(Some(s.to_string())),
				Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid UTF-8")),
			}
		} else {
			Ok(None)
		}
	}
}

impl Encoder for LineCodec {
	type Item = String;
	type Error = io::Error;

	fn encode(&mut self, msg: String, buf: &mut BytesMut) -> io::Result<()> {
		buf.extend_from_slice(msg.as_bytes());
		buf.put(b'\n');
		Ok(())
	}
}

#[cfg(test)]
mod tests {

	use super::LineCodec;

	use server_utils::tokio_io::codec::Decoder;
	use bytes::{BytesMut, BufMut};

	#[test]
	fn simple_encode() {
		let mut buf = BytesMut::with_capacity(2048);
		buf.put_slice(b"{ test: 1 }\n{ test: 2 }\n{ test: 3 }");

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
