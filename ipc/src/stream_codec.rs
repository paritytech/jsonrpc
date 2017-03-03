extern crate tokio_core;

use std::{io, str};
use stream_codec::tokio_core::io::{Codec, EasyBuf};
use validator::extract_requests;

pub struct StreamCodec;

impl Codec for StreamCodec {
	type In = Vec<String>;
	type Out = Vec<String>;

	fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
		let (requests, last_idx) = extract_requests(buf.as_ref());
		buf.drain_to(last_idx);

		Ok(if requests.len() > 0 { Some(requests) } else { None })
	}

	fn encode(&mut self, msgs: Vec<String>, buf: &mut Vec<u8>) -> io::Result<()> {
		for msg in msgs { 
			buf.extend_from_slice(msg.as_bytes());
			buf.push(b'\n');
		}
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

		assert_eq!(request, vec!["{ test: 1 }", "{ test: 2 }", "{ test: 3 }"]);
	}
}
