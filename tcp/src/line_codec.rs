use std::{io, str};
use server_utils::tokio_io::codec::{Decoder, Encoder};
use bytes::BytesMut;

pub struct LineCodec {
	separator: Separator,
}

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
		let mut payload = msg.into_bytes();
		payload.push(b'\n');
		buf.extend_from_slice(&payload);
		Ok(())
	}
}

#[cfg(test)]
mod tests {

	use super::LineCodec;

	use server_utils::tokio_io::codec::Decoder;
	use bytes::{BytesMut, BufMut};


}
