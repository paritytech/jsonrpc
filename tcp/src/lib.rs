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
//! 	let mut io = MetaIoHandler::<()>::new();
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
extern crate futures;
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

#[cfg(test)] mod logger;
#[cfg(test)] mod tests;

pub use server::Server;
pub use meta::{MetaExtractor, RequestContext};
