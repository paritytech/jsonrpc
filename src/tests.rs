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

use jsonrpc_core::IoHandler;
use std::sync::*;

#[cfg(test)]
pub fn dummy_io_handler() -> Arc<IoHandler> {
    use std::sync::Arc;
    use jsonrpc_core::*;

    struct SayHello;
    impl MethodCommand for SayHello {
        fn execute(&self, params: Params) -> Result<Value, Error> {
            let (request_p1, request_p2) = from_params::<(u64, u64)>(params).unwrap();
            let response_str = format!("hello {}! you sent {}", request_p1, request_p2);
            Ok(Value::String(response_str))
        }
    }

    let io = IoHandler::new();
    io.add_method("say_hello", SayHello);
    Arc::new(io)
}

#[cfg(not(windows))]
pub fn dummy_request(addr: &str, buf: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};

    let mut poll = Poll::new().unwrap();
    let mut sock = UnixStream::connect(addr).unwrap();
    poll.register(&sock, Token(0), EventSet::writable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
    poll.poll(Some(500)).unwrap();
    sock.write_all(buf).unwrap();
    poll.reregister(&sock, Token(0), EventSet::readable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
    poll.poll(Some(500)).unwrap();

    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).unwrap_or_else(|_| { 0 });
    buf
}

#[cfg(windows)]
pub fn dummy_request(addr: &str, buf: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
    use miow::pipe::NamedPipe;
    use std::fs::OpenOptions;

    NamedPipe::wait(addr, None).unwrap();
    let mut f = OpenOptions::new().read(true).write(true).open(addr).unwrap();
    f.write_all(buf).unwrap();

    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap_or_else(|_| { 0 });
    buf
}