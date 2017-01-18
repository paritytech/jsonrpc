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

use std::sync::{Arc, Mutex};
use std::str::FromStr;
use std::net::SocketAddr;
use std::thread;

use tokio_core::reactor::{Core, Timeout};
use tokio_core::net::TcpStream;
use tokio_core::io;
use futures::{Future, future};

use jsonrpc::{MetaIoHandler, Value, Metadata};
use Server;
use MetaExtractor;
use RequestContext;

fn casual_server(socket_addr: &SocketAddr) -> Server {
    let mut io = MetaIoHandler::<()>::new();
    io.add_method("say_hello", |_params| {
        Ok(Value::String("hello".to_string()))
    });
    Server::new(socket_addr.clone(), Arc::new(io))
}

fn wait(millis: u64) {
    thread::sleep(::std::time::Duration::from_millis(millis));
}

#[test]
fn doc_test() {
    ::logger::init_log();

    let mut io = MetaIoHandler::<()>::new();
    io.add_method("say_hello", |_params| {
        Ok(Value::String("hello".to_string()))
    });
    let server = Server::new(SocketAddr::from_str("0.0.0.0:17770").unwrap(), Arc::new(io));
    thread::spawn(move || server.run().expect("Server must run with no issues"));
}

#[test]
fn doc_test_connect() {
    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17775".parse().unwrap();
    let server = casual_server(&addr);
    thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let mut core = Core::new().expect("Tokio Core should be created with no errors");
    let stream = TcpStream::connect(&addr, &core.handle());
    let result = core.run(stream);

    assert!(result.is_ok());
}

fn dummy_request(addr: &SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut core = Core::new().expect("Tokio Core should be created with no errors");
    let mut buffer = vec![0u8; 1024];

    let stream = TcpStream::connect(addr, &core.handle())
        .and_then(|stream| {
            io::write_all(stream, data)
        })
        .and_then(|(stream, _)| {
            io::read(stream, &mut buffer)
        })
        .and_then(|(_, read_buf, len)| {
            future::ok(read_buf[0..len].to_vec())
        });
    let result = core.run(stream).expect("Core should run with no errors");

    result
}

fn dummy_request_str(addr: &SocketAddr, data: &[u8]) -> String {
    String::from_utf8(dummy_request(addr, data)).expect("String should be utf-8")
}

#[test]
fn doc_test_handle() {
    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17780".parse().unwrap();
    let server = casual_server(&addr);
    thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let result = dummy_request_str(
        &addr,
        b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n",
    );

    assert_eq!(
        result,
        "{\"jsonrpc\":\"2.0\",\"result\":\"hello\",\"id\":1}\n",
        "Response does not exactly much the expected response",
    );
}

#[derive(Clone)]
pub struct SocketMetadata {
    addr: SocketAddr,
}

impl Default for SocketMetadata {
    fn default() -> Self {
        SocketMetadata { addr: "0.0.0.0:0".parse().unwrap() }
    }
}

impl SocketMetadata {
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }
}

impl Metadata for SocketMetadata { }

impl From<SocketAddr> for SocketMetadata {
    fn from(addr: SocketAddr) -> SocketMetadata {
        SocketMetadata { addr: addr }
    }
}

pub struct PeerMetaExtractor;

impl MetaExtractor<SocketMetadata> for PeerMetaExtractor {
    fn extract(&self, context: &RequestContext) -> SocketMetadata {
        context.peer_addr.into()
    }
}

fn meta_server(socket_addr: &SocketAddr) -> Server<SocketMetadata> {
    let mut io = MetaIoHandler::<SocketMetadata>::new();
    io.add_method_with_meta("say_hello", |_params, meta: SocketMetadata| {
        future::ok(Value::String(format!("hello, {}", meta.addr()))).boxed()
    });
    Server::new(socket_addr.clone(), Arc::new(io)).extractor(Arc::new(PeerMetaExtractor) as Arc<MetaExtractor<SocketMetadata>>)
}

#[test]
fn peer_meta() {
    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17785".parse().unwrap();
    let server = meta_server(&addr);
    thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let result = dummy_request_str(
        &addr,
        b"{\"jsonrpc\": \"2.0\", \"method\": \"say_hello\", \"params\": [42, 23], \"id\": 1}\n"
    );

    // contains random port, so just smoky comparing response length
    assert_eq!(
        59,
        result.len()
    );
}

#[derive(Default)]
pub struct PeerListMetaExtractor {
    peers: Mutex<Vec<SocketAddr>>,
}

impl MetaExtractor<SocketMetadata> for PeerListMetaExtractor {
    fn extract(&self, context: &RequestContext) -> SocketMetadata {
        trace!(target: "tcp", "extracting to peer list...");
        self.peers.lock().unwrap().push(context.peer_addr.clone());
        context.peer_addr.into()
    }
}

#[test]
fn message() {
    ::logger::init_log();
    let addr: SocketAddr = "127.0.0.1:17790".parse().unwrap();
    let mut io = MetaIoHandler::<SocketMetadata>::new();
    io.add_method_with_meta("say_hello", |_params, meta: SocketMetadata| {
        future::ok(Value::String(format!("hello, {}", meta.addr()))).boxed()
    });
    let peer_list = Arc::new(PeerListMetaExtractor::default());
    let server = Server::new(addr.clone(), Arc::new(io))
        .extractor(peer_list.clone() as Arc<MetaExtractor<SocketMetadata>>);
    let dispatcher = server.dispatcher();

    thread::spawn(move || server.run().expect("Server must run with no issues"));
    wait(100);

    let mut core = Core::new().expect("Tokio Core should be created with no errors");
    let timeout = Timeout::new(::std::time::Duration::from_millis(100), &core.handle())
        .expect("There should be a timeout produced in message test");
    let mut buffer = vec![0u8; 1024];
    let stream = TcpStream::connect(&addr, &core.handle())
        .and_then(|stream| {
            future::ok(stream).join(timeout)
        })
        .and_then(|stream| {
            let peer_addr = peer_list.peers.lock().unwrap()[0].clone();
            dispatcher.push_message(
                &peer_addr,
                "ping".to_owned(),
            );
            trace!(target: "tcp", "Dispatched message for {}", peer_addr);
            future::ok(stream)
        })
        .and_then(|(stream, _)| {
            io::read(stream, &mut buffer)
        })
        .and_then(|(_, read_buf, len)| {
            trace!(target: "tcp", "Read message");
            future::ok(read_buf[0..len].to_vec())
        });

    let result = core.run(stream)
        .expect("Should be the payload in message test");

    assert_eq!(
        "ping\n",
        String::from_utf8(result).expect("String should be utf-8"),
        "Sent request does not match received by the peer",
    );
}
