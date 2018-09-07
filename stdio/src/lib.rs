//! jsonrpc server using stdin/stdout
//!
//! ```no_run
//! extern crate jsonrpc_stdio_server;
//!
//! use jsonrpc_stdio_server::ServerBuilder;
//! use jsonrpc_stdio_server::jsonrpc_core::*;
//!
//! fn main() {
//! 	let mut io = IoHandler::default();
//! 	io.add_method("say_hello", |_params| {
//! 		Ok(Value::String("hello".to_owned()))
//! 	});
//!
//! 	ServerBuilder::new(io).build();
//! }
//! ```

#![warn(missing_docs)]

extern crate futures;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;
extern crate tokio_stdin_stdout;
#[macro_use] extern crate log;

pub extern crate jsonrpc_core;

use std::sync::Arc;
use tokio::prelude::{Future, Stream};
use tokio_codec::{FramedRead, FramedWrite, LinesCodec};
use jsonrpc_core::{IoHandler};

/// Stdio server builder
pub struct ServerBuilder {
	handler: Arc<IoHandler>,
}

impl ServerBuilder {
	/// Returns a new server instance
	pub fn new<T>(handler: T) -> Self where T: Into<IoHandler> {
		ServerBuilder { handler: Arc::new(handler.into()) }
	}

	/// Will block until EOF is read or until an error occurs.
	/// The server reads from STDIN line-by-line, one request is taken
	/// per line and each response is written to STDOUT on a new line.
	pub fn build(&self) {
		let stdin = tokio_stdin_stdout::stdin(0);
		let stdout = tokio_stdin_stdout::stdout(0).make_sendable();

		let framed_stdin = FramedRead::new(stdin, LinesCodec::new());
		let framed_stdout = FramedWrite::new(stdout, LinesCodec::new());

		let handler = self.handler.clone();
		let future = framed_stdin
			.and_then(move |line| process(&handler, line).map_err(|_| unreachable!()))
			.forward(framed_stdout)
			.map(|_| ())
			.map_err(|e| panic!("{:?}", e));

		tokio::run(future);
	}
}

/// Process a request asynchronously
fn process(io: &Arc<IoHandler>, input: String) -> impl Future<Item = String, Error = ()> + Send {
	io.handle_request(&input).map(move |result| match result {
		Some(res) => res,
		None => {
			info!("JSON RPC request produced no response: {:?}", input);
			String::from("")
		}
	})
}
