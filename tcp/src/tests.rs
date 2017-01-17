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

use std::sync::Arc;
use std::str::FromStr;
use std::net::SocketAddr;

use jsonrpc::{MetaIoHandler, Value};
use Server;
use SocketMetadata;

fn casual_server(socket_addr: &SocketAddr) -> Server {
    let mut io = MetaIoHandler::<SocketMetadata>::new();
    io.add_method("say_hello", |_params| {
        Ok(Value::String("hello".to_string()))
    });
    Server::new(socket_addr.clone(), Arc::new(io))
}

fn wait(millis: u64) {
    ::std::thread::sleep(::std::time::Duration::from_millis(millis));
}

#[test]
fn doc_test() {
    ::logger::init_log();

    let mut io = MetaIoHandler::<SocketMetadata>::new();
    io.add_method("say_hello", |_params| {
        Ok(Value::String("hello".to_string()))
    });
    let server = Server::new(SocketAddr::from_str("0.0.0.0:17770").unwrap(), Arc::new(io));
    ::std::thread::spawn(move || server.run().expect("Server must run with no issues"));
}

#[test]
fn doc_test_connect() {
    use tokio_core::reactor::Core;
    use tokio_core::net::TcpStream;

    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17775".parse().unwrap();
    let server = casual_server(&addr);
    ::std::thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let mut core = Core::new().expect("Tokio Core should be created with no errors");
    let stream = TcpStream::connect(&addr, &core.handle());
    let result = core.run(stream);

    assert!(result.is_ok());
}


#[test]
fn doc_test_handle() {
    use tokio_core::reactor::Core;
    use tokio_core::net::TcpStream;
    use tokio_core::io;
    use futures::{Future, future};

    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17780".parse().unwrap();
    let server = casual_server(&addr);
    ::std::thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let mut core = Core::new().expect("Tokio Core should be created with no errors");
    let mut buffer = vec![0u8; 1024];

    let stream = TcpStream::connect(&addr, &core.handle())
        .and_then(|stream| {
            let data = b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n";

            io::write_all(stream, &data[..])
        })
        .and_then(|(stream, _)| {
            io::read(stream, &mut buffer)
        })
        .and_then(|(_, read_buf, len)| {
            assert_eq!(
                "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
                String::from_utf8(read_buf[0..len].to_vec()).unwrap(),
                "Response does not much the expected one",
            );
            future::ok(())
        });
    let result = core.run(stream);

    assert!(result.is_ok());
}
