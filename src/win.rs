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

//! jsonrpc server over win named pipes
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

//! Named pipes library

use miow::pipe::{NamedPipe, NamedPipeBuilder};
use std;
use std::io;
use std::io::{Read, Write};
use std::sync::atomic::*;
use std::sync::Arc;
use jsonrpc_core::IoHandler;

pub type Result<T> = std::result::Result<T, Error>;

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

pub struct PipeHandler {
    waiting_pipe: NamedPipe,
    io_handler: Arc<IoHandler>,
}

impl PipeHandler {
    /// start ipc rpc server (blocking)
    pub fn start(addr: &str, io_handler: &Arc<IoHandler>) -> Result<PipeHandler> {
        Ok(PipeHandler {
            waiting_pipe: try!(NamedPipe::new(addr)),
            io_handler: io_handler.clone(),
        })
    }

    fn handle_incoming(&mut self, addr: &str) -> io::Result<()> {
        try!(self.waiting_pipe.connect());
        let mut connected_pipe = std::mem::replace::<NamedPipe>(&mut self.waiting_pipe,
            try!(NamedPipeBuilder::new(addr)
                .first(false)
                .inbound(true)
                .outbound(true)
                .out_buffer_size(65536)
                .in_buffer_size(65536)
                .create()));

        let thread_handler = self.io_handler.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            match connected_pipe.read_to_end(&mut buf) {
                Ok(_) => {
                    if let Err(parse_err) = String::from_utf8(buf)
                    .map(|rpc_msg| {
                        let response: Option<String> = thread_handler.handle_request(&rpc_msg);
                        if let Some(response_str) = response {
                            let response_bytes = response_str.into_bytes();
                            if let Err(write_err) = connected_pipe.write(&response_bytes[..]) {
                                // todo : no stable logging for windows?
                                println!("Response write error: {:?}", write_err);
                            }
                        }
                    })
                    {
                        // todo : no stable logging for windows?
                        println!("Response decode error: {:?}", parse_err);
                    }
                },
                Err(read_err) => {
                    // todo : no stable logging for windows?
                    println!("Response decode error: {:?}", read_err);
                }
            }
        });

        Ok(())
    }
}

pub struct Server {
    is_stopping: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    addr: String,
    io_handler: Arc<IoHandler>,
}

impl Server {
    /// New server
    pub fn new(socket_addr: &str, io_handler: &Arc<IoHandler>) -> Result<Server> {
        Ok(Server {
            io_handler: io_handler.clone(),
            is_stopping: Arc::new(AtomicBool::new(false)),
            is_stopped: Arc::new(AtomicBool::new(true)),
            addr: socket_addr.to_owned(),
        })
    }

    /// Run server (in this thread)
    pub fn run(&self) -> Result<()> {
        let mut pipe_handler = try!(PipeHandler::start(&self.addr, &self.io_handler));
        loop  {
            try!(pipe_handler.handle_incoming(&self.addr))
        }
    }

    /// Run server (in separate thread)
    pub fn run_async(&self) -> Result<()> {
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::IsStopping) }
        if !self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStopped) }

        let thread_stopping = self.is_stopping.clone();
        let thread_stopped = self.is_stopped.clone();
        let thread_handler = self.io_handler.clone();
        let addr = self.addr.clone();
        std::thread::spawn(move || {
            let mut pipe_handler = PipeHandler::start(&addr, &thread_handler).unwrap();
            while !thread_stopping.load(Ordering::Relaxed) {
                if let Err(pipe_listener_error) = pipe_handler.handle_incoming(&addr) {
                    // todo : no stable logging for windows?
                    println!("Pipe listening error: {:?}", pipe_listener_error);
                }
            }
            thread_stopped.store(true, Ordering::Relaxed);
        });

        self.is_stopped.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop_async(&self) -> Result<()> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
        if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
        self.is_stopping.store(true, Ordering::Relaxed);
        while !self.is_stopped.load(Ordering::Relaxed) { std::thread::park_timeout(std::time::Duration::new(0, 50)); }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop().unwrap_or_else(|_| {}); // ignore error - can be stopped already
    }
}


#[cfg(test)]
mod tests {

    use rand::{thread_rng, Rng};
    use std;
    use tests::dummy_request;
    use super::Server;
    use tests;

    fn random_pipe_name() -> String {
        let name = thread_rng().gen_ascii_chars().take(30).collect::<String>();
        format!(r"\\.\pipe\{}", name)
    }


    #[test]
    pub fn test_reqrep_poll() {
        let addr = &random_pipe_name();
        let io = tests::dummy_io_handler();
        let server = Server::new(addr, &io).unwrap();
        server.run_async().unwrap();

        let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
        let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
        assert_eq!(String::from_utf8(dummy_request(addr, request.as_bytes())).unwrap(), response.to_string());

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}