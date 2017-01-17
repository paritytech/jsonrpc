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

use std::io;
use std::str;
use tokio_core::io::{Codec, EasyBuf};

pub struct LineCodec;

impl Codec for LineCodec {
    type In = String;
    type Out = String;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
        if let Some(i) = buf.as_slice().iter().position(|&b| b == b'\n') {
            let line = buf.drain_to(i);
            buf.drain_to(1);
            return match str::from_utf8(&line.as_ref()) {
                Ok(s) => Ok(Some(s.to_string())),
                Err(_) => Err(io::Error::new(io::ErrorKind::Other,
                                             "invalid UTF-8")),
            }
        } else {
            Ok(None)
        }
    }

    fn encode(&mut self, msg: String, buf: &mut Vec<u8>)
         -> io::Result<()>
    {
        buf.extend_from_slice(msg.as_bytes());
        buf.push(b'\n');
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::LineCodec;
    use tokio_core::io::{Codec, EasyBuf};

    #[test]
    fn simple_encode() {
        let mut buf = EasyBuf::new();
        buf.get_mut().extend_from_slice(b"{ test: 1 }\n{ test: 2 }");

        let mut codec = LineCodec;

        let request = codec.decode(&mut buf)
            .expect("There should be no error in simple test")
            .expect("There should be at least one request in simple test");

        assert_eq!(request, "{ test: 1 }")
    }
}
