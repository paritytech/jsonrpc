//! ### Transport agnostic jsonrpc library.
//!
//! Right now it supports only server side handling requests.
//!
//! ```rust
//! extern crate jsonrpc_core;
//!
//! use jsonrpc_core::*;
//!
//! struct SayHello;
//! impl SyncMethodCommand for SayHello {
//!     fn execute(&self, _params: Params) -> Result<Value, Error> {
//!         Ok(Value::String("hello".to_string()))
//!     }
//! }
//!
//! struct SayHelloAsync;
//! impl MethodCommand for SayHelloAsync {
//!		fn execute(&self, _params: Params, ready: Ready) {
//!			ready.ready(Ok(Value::String("hello".to_string())))
//!		}
//! }
//!
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//! 	io.add_async_method("say_hello_async", SayHelloAsync);
//!
//! 	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
//! 	let request2 = r#"{"jsonrpc": "2.0", "method": "say_hello_async", "params": [42, 23], "id": 1}"#;
//! 	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;
//!
//! 	assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
//! 	assert_eq!(io.handle_request_sync(request2), Some(response.to_string()));
//! }
//! ```

#![warn(missing_docs)]
#![cfg_attr(feature="nightly", feature(custom_derive, plugin))]
#![cfg_attr(feature="nightly", plugin(serde_macros))]

extern crate serde;
extern crate serde_json;
extern crate parking_lot;
#[macro_use] extern crate log;

// pub mod async;
pub mod control;
pub mod commander;
pub mod request_handler;
pub mod io;

pub use self::control::*;
pub use self::commander::{Commander, MethodCommand, SyncMethodCommand, NotificationCommand, SubscriptionCommand};
pub use self::request_handler::RequestHandler;
pub use self::io::{IoHandler, IoDelegate, IoSession, IoSessionHandler};

#[cfg(feature = "serde_macros")]
include!("lib.rs.in");

#[cfg(not(feature = "serde_macros"))]
include!(concat!(env!("OUT_DIR"), "/lib.rs"));
