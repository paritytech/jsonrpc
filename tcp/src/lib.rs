//! jsonrpc server over tcp/ip
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_tcp_server;
//! extern crate rand;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use jsonrpc_tcp_server::Server;
//! use std::net::SocketAddr;
//! use std::str::FromStr;
//!
//! fn main() {
//! 	let mut io = MetaIoHandler::<()>::default();
//! 	io.add_method("say_hello", |_params| {
//! 		Ok(Value::String("hello".to_string()))
//! 	});
//! 	let server = Server::new(SocketAddr::from_str("0.0.0.0:9993").unwrap(), Arc::new(io));
//!     ::std::thread::spawn(move || server.run().expect("Server must run with no issues"));
//! }
//! ```

extern crate jsonrpc_core as jsonrpc;
extern crate serde_json;
extern crate rand;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;

#[macro_use] extern crate log;
#[cfg(test)] #[macro_use] extern crate lazy_static;
#[cfg(test)] extern crate env_logger;

mod line_codec;
mod service;
mod server;
mod meta;
mod dispatch;

#[cfg(test)] mod logger;
#[cfg(test)] mod tests;

pub use server::Server;
pub use dispatch::{Dispatcher, PushMessageError};
pub use meta::{MetaExtractor, RequestContext};
