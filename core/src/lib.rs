//! ### Transport agnostic jsonrpc library.
//!
//! Right now it supports only server side handling requests.
//!
//! ```rust
//! use jsonrpc_core::*;
//!
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.add_sync_method("say_hello", |_| {
//!			Ok(Value::String("Hello World!".into()))
//! 	});
//!
//! 	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
//! 	let response = r#"{"jsonrpc":"2.0","result":"Hello World!","id":1}"#;
//!
//! 	assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
//! }
//! ```

#![deny(missing_docs)]

use std::pin::Pin;

#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

pub use futures;

#[doc(hidden)]
pub extern crate serde;
#[doc(hidden)]
pub extern crate serde_json;

mod calls;
mod io;

pub mod delegates;
pub mod middleware;
pub mod types;

/// A Result type.
pub type Result<T> = std::result::Result<T, Error>;

/// A `Future` trait object.
pub type BoxFuture<T> = Pin<Box<dyn futures::Future<Output = T> + Send>>;

pub use crate::calls::{
	Metadata, RemoteProcedure, RpcMethod, RpcMethodSimple, RpcMethodSync, RpcNotification, RpcNotificationSimple,
	WrapFuture,
};
pub use crate::delegates::IoDelegate;
pub use crate::io::{
	Compatibility, FutureOutput, FutureResponse, FutureResult, FutureRpcResult, IoHandler, IoHandlerExtension,
	MetaIoHandler,
};
pub use crate::middleware::{Middleware, Noop as NoopMiddleware};
pub use crate::types::*;

use serde_json::Error as SerdeError;

/// workaround for https://github.com/serde-rs/json/issues/505
/// Arbitrary precision confuses serde when deserializing into untagged enums,
/// this is a workaround
pub fn serde_from_str<'a, T>(input: &'a str) -> std::result::Result<T, SerdeError>
where
	T: serde::de::Deserialize<'a>,
{
	if cfg!(feature = "arbitrary_precision") {
		let val = serde_json::from_str::<Value>(input)?;
		T::deserialize(val)
	} else {
		serde_json::from_str::<T>(input)
	}
}
