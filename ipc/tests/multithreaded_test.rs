extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server;
extern crate log;
extern crate env_logger;
#[macro_use] extern crate lazy_static;
extern crate miow;

use std::env;
use log::LogLevelFilter;
use env_logger::LogBuilder;
use jsonrpc_core::*;
use jsonrpc_core::futures::Future;
use jsonrpc_core::futures::future::ok;
use jsonrpc_ipc_server::Server;

#[cfg(windows)]
use miow::pipe::connect;
#[cfg(not(windows))]
use std::os::unix::net::UnixStream;

use std::thread;
use std::time::Duration;
use std::io::{Read, Write};

lazy_static! {
	static ref LOG_DUMMY: bool = {
		let mut builder = LogBuilder::new();
		builder.filter(None, LogLevelFilter::Info);

		if let Ok(log) = env::var("RUST_LOG") {
			builder.parse(&log);
		}

		if let Ok(_) = builder.init() {
			println!("logger initialized");
		}
		true
	};
}

/// Intialize log with default settings
pub fn init_log() {
	let _ = *LOG_DUMMY;
}

#[cfg(windows)]
fn pipe_name() -> &'static str {
    "\\\\.\\pipe\\Foo\\Bar\\Baz"
}
#[cfg(not(windows))]
fn pipe_name() -> &'static str {
    "/tmp/foobar.sock"
}

#[cfg(windows)]
fn get_connection(addr: &str) -> ::std::fs::File {
    connect(addr).expect("Failed to get a client connection to the pipe")
}

#[cfg(not(windows))]
fn get_connection(addr: &str) -> UnixStream {
    UnixStream::connect(pipe_name).expect("Failed to connect to unix socket")
}

fn say_to_pipe(addr: &str, message: String) -> String {
    let mut connection = get_connection(addr);

    connection
        .write_all(message.as_bytes())
        .expect("Failed to write to the pipe");

    let mut buf = [0u8; 1024];
    connection
        .read(&mut buf)
        .expect("Failed to read from the pipe");
    String::from_utf8_lossy(&buf)
        .into_owned()
        .trim_right_matches('\u{0}')
        .to_string()
}


fn message(n: i32) -> String {
    format!(r#"{{ "jsonrpc":"2.0", "method":"hello", "params": {{"message": "Hello from {n}!"}}, "id": {n} }}"#, n=n)
}

fn expected_response(n: i32) -> String {
    format!(r#"{{"jsonrpc":"2.0","result":"hello accepted","id":{n}}}"#,
            n = n)
}

#[test]
fn processes_several_requests_at_once() {
    init_log();

    let mut io = IoHandler::new();
    io.add_async_method("hello", |_params| {
        ok(String::new())
            .and_then(|x| {
                            thread::sleep(Duration::from_millis(100));
                            ok(x)
                        })
            .and_then(|_| ok(Value::String("hello accepted".into())))
            .boxed()
    });

    let server = Server::new(pipe_name(), io).unwrap();
    thread::spawn(move || server.run());

    thread::sleep(Duration::from_millis(100)); // Let's make sure the pipe server has been initialized

    let thread1 = thread::spawn(|| {
        assert_eq!(say_to_pipe(pipe_name(), message(1)),
        expected_response(1))
    });

    let thread2 = thread::spawn(|| {
        assert_eq!(say_to_pipe(pipe_name(), message(2)),
        expected_response(2))
    });

    thread1.join().unwrap();
    thread2.join().unwrap();
}
