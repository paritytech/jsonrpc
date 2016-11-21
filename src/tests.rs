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
use super::Server;
use std;
use rand::{thread_rng, Rng};
use std::net::SocketAddr;

pub fn dummy_io_handler() -> Arc<IoHandler> {
    use std::sync::Arc;
    use jsonrpc_core::*;

    struct SayHello;
    impl SyncMethodCommand for SayHello {
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

pub fn dummy_request(addr: &SocketAddr, buf: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
	use mio::*;
	use mio::tcp::*;

    let mut poll = Poll::new().unwrap();
    let mut sock = TcpStream::connect(addr).unwrap();
    poll.register(&sock, Token(0), EventSet::writable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
    poll.poll(Some(500)).unwrap();
    sock.write_all(buf).unwrap();
    poll.reregister(&sock, Token(0), EventSet::readable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
    poll.poll(Some(500)).unwrap();

    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).unwrap_or_else(|_| { 0 });
    buf
}

pub fn random_endpoint() -> SocketAddr {
    use std::str::FromStr;

    let port;
    loop {
        let nport = (thread_rng().next_u32() % 65536) as u16;
        if nport > 1000 { port = nport; break }
    }
    SocketAddr::from_str(&format!("0.0.0.0:{}", port)).unwrap()
}

#[test]
pub fn test_reqrep() {
    let addr = random_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();
    std::thread::park_timeout(std::time::Duration::from_millis(50));

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#.to_owned() + "\n";

    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response);
}

#[test]
pub fn test_sequental_connections() {
    super::init_log();

    let addr = random_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();
    std::thread::park_timeout(std::time::Duration::from_millis(50));

    for i in 0..100 {
        let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42,"#.to_owned() + &format!("{}", i) + r#"], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent "#.to_owned() + &format!("{}", i)  + r#"","id":1}"# + "\n";
        assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response);
    }
}

#[test]
pub fn test_reqrep_poll() {
    let addr = random_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    std::thread::spawn(move || {
        loop {
            server.poll();
        }
    });

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#.to_owned() + "\n";
    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response);

    std::thread::sleep(std::time::Duration::from_millis(500));
}
