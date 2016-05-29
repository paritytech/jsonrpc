//! jsonrpc server over unix sockets (todo: ... and later pipes)
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate json_ipc_server;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use json_ipc_server::Server;
//!
//! struct SayHello;
//! impl MethodCommand for SayHello {
//! 	fn execute(&self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//!
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	let server = Server::new("/tmp/json-ipc-test.ipc", &Arc::new(io)).unwrap();
//!     ::std::thread::spawn(move || server.run());
//! }
//! ```

extern crate mio;
extern crate jsonrpc_core;
extern crate bytes;
extern crate slab;

use mio::*;
use mio::unix::*;
use bytes::{Buf, ByteBuf, MutByteBuf};
use std::io;
use jsonrpc_core::IoHandler;
use std::sync::*;
use std::sync::atomic::*;

const SERVER: Token = Token(0);
const MAX_CONCURRENT_CONNECTIONS: usize = 16;

struct SocketConnection {
    socket: UnixStream,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>,
    token: Option<Token>,
    interest: EventSet,
}

type Slab<T> = slab::Slab<T, Token>;

impl SocketConnection {
    fn new(sock: UnixStream) -> Self {
        SocketConnection {
            socket: sock,
            buf: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(2048)),
            token: None,
            interest: EventSet::hup(),
        }
    }

    fn writable(&mut self, event_loop: &mut EventLoop<RpcServer>, _handler: &IoHandler) -> io::Result<()> {
        use std::io::Write;
        if let Some(buf) = self.buf.take() {
            try!(self.socket.write_all(&buf.bytes()));
        }

        self.interest.remove(EventSet::writable());
        self.interest.insert(EventSet::readable());

        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }

    fn readable(&mut self, event_loop: &mut EventLoop<RpcServer>, handler: &IoHandler) -> io::Result<()> {
        let mut buf = self.mut_buf.take().unwrap_or_else(|| panic!("unwrapping mutable buffer which is None"));

        match self.socket.try_read_buf(&mut buf) {
            Ok(None) => {
                self.mut_buf = Some(buf);
            }
            Ok(Some(_)) => {
                String::from_utf8(buf.bytes().to_vec())
                    .map(|rpc_msg| {
                        let response: Option<String> = handler.handle_request(&rpc_msg);
                        if let Some(response_str) = response {
                            let response_bytes = response_str.into_bytes();
                            self.buf = Some(ByteBuf::from_slice(&response_bytes));
                        }
                    }).unwrap();

                self.mut_buf = Some(ByteBuf::mut_with_capacity(2048));

                self.interest.remove(EventSet::readable());
                self.interest.insert(EventSet::writable());
            }
            Err(_) => {
                //warn!(target: "ipc", "Error receiving data: {:?}", e);
                self.interest.remove(EventSet::readable());
            }

        };

        event_loop.reregister(&self.socket, self.token.unwrap(), self.interest, PollOpt::edge() | PollOpt::oneshot())
    }
}

struct RpcServer {
    socket: UnixListener,
    connections: Slab<SocketConnection>,
    io_handler: Arc<IoHandler>,
}

pub struct Server {
    rpc_server: Arc<RwLock<RpcServer>>,
    event_loop: Arc<RwLock<EventLoop<RpcServer>>>,
    is_stopping: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    addr: String,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotStarted,
    AlreadyStopping,
    NotStopped,
    IsStopping,
}

impl std::convert::From<std::io::Error> for Error {
    fn from(io_error: std::io::Error) -> Error {
        Error::Io(io_error)
    }
}

impl Server {
    /// New server
    pub fn new(socket_addr: &str, io_handler: &Arc<IoHandler>) -> Result<Server, Error> {
        let (server, event_loop) = try!(RpcServer::start(socket_addr, io_handler));
        Ok(Server {
            rpc_server: Arc::new(RwLock::new(server)),
            event_loop: Arc::new(RwLock::new(event_loop)),
            is_stopping: Arc::new(AtomicBool::new(false)),
            is_stopped: Arc::new(AtomicBool::new(true)),
            addr: socket_addr.to_owned(),
        })
    }

    /// Run server (in current thread)
    pub fn run(&self) {
        let mut event_loop = self.event_loop.write().unwrap();
        let mut server = self.rpc_server.write().unwrap();
        event_loop.run(&mut server).unwrap();
    }

    /// Poll server requests (for manual async scenarios)
    pub fn poll(&self) {
        let mut event_loop = self.event_loop.write().unwrap();
        let mut server = self.rpc_server.write().unwrap();

        event_loop.run_once(&mut server, Some(100)).unwrap();
    }

    /// Run server (in separate thread)
    pub fn run_async(&self) -> Result<(), Error> {
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::IsStopping) }
        if !self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStopped) }

        self.is_stopped.store(false, Ordering::Relaxed);

        let event_loop = self.event_loop.clone();
        let server = self.rpc_server.clone();
        let thread_stopping = self.is_stopping.clone();
        let thread_stopped = self.is_stopped.clone();
        std::thread::spawn(move || {
            let mut event_loop = event_loop.write().unwrap();
            let mut server = server.write().unwrap();
            while !thread_stopping.load(Ordering::Relaxed) {
                event_loop.run_once(&mut server, Some(100)).unwrap();
            }
            thread_stopped.store(true, Ordering::Relaxed);
        });
        Ok(())
    }

    pub fn stop_async(&self) -> Result<(), Error> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        while !self.is_stopped.load(Ordering::Relaxed) { std::thread::sleep(std::time::Duration::new(0, 10)); }
        Ok(())
    }
}


impl Drop for Server {
    fn drop(&mut self) {
        self.stop().unwrap_or_else(|_| {}); // ignore error - can be stopped already
        ::std::fs::remove_file(&self.addr).unwrap_or_else(|_| {}); // ignoer error - server could have never been started
    }
}

impl RpcServer {

    /// start ipc rpc server (blocking)
    pub fn start(addr: &str, io_handler: &Arc<IoHandler>) -> Result<(RpcServer, EventLoop<RpcServer>), Error> {
        let mut event_loop = try!(EventLoop::new());
        ::std::fs::remove_file(addr).unwrap_or_else(|_| {} ); // ignore error (if no file)
        let socket = try!(UnixListener::bind(&addr));
        event_loop.register(&socket, SERVER, EventSet::readable(), PollOpt::edge()).unwrap();
        let server = RpcServer {
            socket: socket,
            connections: Slab::new_starting_at(Token(1), MAX_CONCURRENT_CONNECTIONS),
            io_handler: io_handler.clone(),
        };
        Ok((server, event_loop))
    }

    fn accept(&mut self, event_loop: &mut EventLoop<RpcServer>) -> io::Result<()> {
        let new_client_socket = self.socket.accept().unwrap().unwrap();
        let connection = SocketConnection::new(new_client_socket);
        if self.connections.count() >= MAX_CONCURRENT_CONNECTIONS {
            let oldest_token = self.connections.iter().nth(0)
                .expect("fatal: impossible since count > MAX_CONCURRENT_CONNECTIONS and we request first one")
                .token.unwrap().clone();
            self.connections.remove(oldest_token);
        }
        let token = self.connections.insert(connection).ok().expect("fatal: Could not add connection to slab (memory issue?)");

        self.connections[token].token = Some(token);
        event_loop.register(
            &self.connections[token].socket,
            token,
            EventSet::readable(),
            PollOpt::edge() | PollOpt::oneshot()
        ).ok().expect("fatal: could not register socket with event loop (memory issue?)");

        Ok(())
    }

    fn connection_readable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        let io_handler = self.io_handler.clone();
        self.connection(tok).readable(event_loop, &io_handler)
    }

    fn connection_writable(&mut self, event_loop: &mut EventLoop<RpcServer>, tok: Token) -> io::Result<()> {
        let io_handler = self.io_handler.clone();
        self.connection(tok).writable(event_loop, &io_handler)
    }

    fn connection<'a>(&'a mut self, tok: Token) -> &'a mut SocketConnection {
        &mut self.connections[tok]
    }
}


impl Handler for RpcServer {
    type Timeout = usize;
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<RpcServer>, token: Token, events: EventSet) {
        if events.is_readable() {
            match token {
                SERVER => self.accept(event_loop).unwrap(),
                _ => self.connection_readable(event_loop, token).unwrap()
            };
        }

        if events.is_writable() {
            match token {
                SERVER => { },
                _ => self.connection_writable(event_loop, token).unwrap_or_else(|_| {}), // todo: disqualify connection from list on error
            };
        }
    }
}

#[cfg(test)]
fn dummy_request(addr: &str, buf: &[u8]) -> Vec<u8> {
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

#[cfg(test)]
fn dummy_io_handler() -> Arc<IoHandler> {
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

#[test]
pub fn test_reqrep() {
    let addr = "/tmp/test10";
    let io = dummy_io_handler();
    let server = Server::new(addr, &io).unwrap();
    std::thread::spawn(move || {
        server.run()
    });

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;

    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
pub fn test_reqrep_two_sequental_connections() {
    let addr = "/tmp/test15";
    let io = dummy_io_handler();
    let server = Server::new(addr, &io).unwrap();
    std::thread::spawn(move || {
        server.run()
    });

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 555! you sent 666","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());

    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
pub fn test_reqrep_three_sequental_connections() {
    let addr = "/tmp/test25";
    let io = dummy_io_handler();
    let server = Server::new(addr, &io).unwrap();
    std::thread::spawn(move || {
        server.run()
    });

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
    String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap();

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [9999, 1111], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 9999! you sent 1111","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
pub fn test_reqrep_100_connections() {
    let addr = "/tmp/test45";
    let io = dummy_io_handler();
    let server = Server::new(addr, &io).unwrap();
    std::thread::spawn(move || {
        server.run()
    });

    for i in 0..100 {
        let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42,"#.to_owned() + &format!("{}", i) + r#"], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent "#.to_owned() + &format!("{}", i)  + r#"","id":1}"#;
        assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());
    }
}

#[test]
pub fn test_reqrep_poll() {
    let addr = "/tmp/test40";
    let io = dummy_io_handler();
    let server = Server::new(addr, &io).unwrap();
    std::thread::spawn(move || {
        loop {
            server.poll();
        }
    });

    let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
    let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
    assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());

    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
pub fn test_file_removed() {
    let addr = "/tmp/test50";
    let io = dummy_io_handler();
    {
        let server = Server::new(addr, &io).unwrap();
        server.run_async().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
        assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());
    }
    assert!(::std::fs::metadata(addr).is_err()); // err is file not exists
}