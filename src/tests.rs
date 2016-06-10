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
	use mio::*;
	use mio::unix::*;

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
    f.flush().unwrap();

    let mut buf = vec![0u8; 65536];
    let sz = f.read(&mut buf).unwrap_or_else(|_| { 0 });
    (&buf[0..sz]).to_vec()
}


pub fn random_ipc_endpoint() -> String {
    let name = thread_rng().gen_ascii_chars().take(30).collect::<String>();
    if cfg!(windows) {
        format!(r"\\.\pipe\{}", name)
    }
    else {
        format!(r"/tmp/{}", name)
    }
}

#[test]
pub fn test_reqrep() {
    let addr = random_ipc_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();


    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;

    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
pub fn test_reqrep_two_sequental_connections() {
    let addr = random_ipc_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 555! you sent 666","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
pub fn test_reqrep_three_sequental_connections() {
    let addr = random_ipc_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
    String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap();

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [9999, 1111], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 9999! you sent 1111","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
pub fn test_reqrep_100_connections() {
    let addr = random_ipc_endpoint();
    let io = dummy_io_handler();
    let server = Server::new(&addr, &io).unwrap();
    server.run_async().unwrap();

    for i in 0..100 {
        let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42,"#.to_owned() + &format!("{}", i) + r#"], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent "#.to_owned() + &format!("{}", i)  + r#"","id":1}"#;
        assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
    }
}
