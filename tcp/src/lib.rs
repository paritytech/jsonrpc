//! jsonrpc server over tcp/ip
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_tcp_server;
//! extern crate rand;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_tcp_server::ServerBuilder;
//!
//! fn main() {
//! 	let mut io = IoHandler::default();
//! 	io.add_method("say_hello", |_params| {
//! 		Ok(Value::String("hello".to_string()))
//! 	});
//! 	let server = ServerBuilder::new(io)
//!			.start(&"0.0.0.0:0".parse().unwrap())
//!			.expect("Server must start with no issues.");
//!
//!		server.wait().unwrap();
//! }
//! ```

#![warn(missing_docs)]

extern crate jsonrpc_core as jsonrpc;
extern crate parking_lot;
extern crate rand;
extern crate serde_json;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;

#[macro_use] extern crate log;

#[cfg(test)] #[macro_use] extern crate lazy_static;
#[cfg(test)] extern crate env_logger;

mod dispatch;
mod line_codec;
mod meta;
mod server;
mod service;

#[cfg(test)] mod logger;
#[cfg(test)] mod tests;

pub use dispatch::{Dispatcher, PushMessageError};
pub use meta::{MetaExtractor, RequestContext};
pub use server::ServerBuilder;
