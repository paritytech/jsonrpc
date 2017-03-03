extern crate tokio_core;

use std::io;
use stream_codec::tokio_core::io::{Codec, EasyBuf};
use validator::extract_requests;

pub struct StreamCodec;

impl Codec for StreamCodec {
	type In = Vec<String>;
	type Out = Vec<String>;

	fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
		let (requests, last_idx) = extract_requests(buf.as_ref());
		Ok(if requests.len() > 0 { buf.drain_to(last_idx+1); Some(requests) } else { None })
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

	#[test]
	fn fragmented_encode() {
		let mut buf = EasyBuf::new();
		// TODO: maybe ignore new lines here also if the output is enveloped with those
		buf.get_mut().extend_from_slice(b"{ test: 1 }{ test: 2 }{ tes");

		let mut codec = StreamCodec;
		let request = codec.decode(&mut buf)
			.expect("There should be no error in first fragmented test")
			.expect("There should be at least one request in first fragmented test");
		assert_eq!(request, vec!["{ test: 1 }", "{ test: 2 }"]);
		assert_eq!(String::from_utf8(buf.as_ref().to_vec()).unwrap(), "{ tes");

		buf.get_mut().extend_from_slice(b"t: 3 }");
		let request = codec.decode(&mut buf)
			.expect("There should be no error in second fragmented test")
			.expect("There should be at least one request in second fragmented test");
		assert_eq!(request, vec!["{ test: 3 }"]);
	}
}
