//! ### Transport agnostic jsonrpc library.
//!
//! Right now it supports only server side handling requests.
//!
//! ```rust
//! extern crate jsonrpc_core;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_core::futures::Future;
//!
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.add_method("say_hello", |_| {
//!			Ok(Value::String("Hello World!".into()))
//! 	});
//!
//! 	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
//! 	let response = r#"{"jsonrpc":"2.0","result":"Hello World!","id":1}"#;
//!
//! 	assert_eq!(io.handle_request(request).wait().unwrap(), Some(response.to_string()));
//! }
//! ```

#![warn(missing_docs)]

#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate serde_json;

pub extern crate futures;

mod calls;
mod io;

mod middleware;
pub mod types;

pub use calls::{RemoteProcedure, Metadata, RpcMethodSync, RpcMethodSimple, RpcMethod, RpcNotificationSimple, RpcNotification};
pub use io::{Compatibility, IoHandler, MetaIoHandler, FutureResponse};
pub use middleware::{Middleware, Noop as NoopMiddleware};
pub use types::*;
