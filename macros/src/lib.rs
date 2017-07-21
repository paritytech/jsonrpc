//! High level, typed wrapper for `jsonrpc_core`.
//!
//! Enables creation of "Service" objects grouping a set of RPC methods together in a typed manner.
//!
//! Example
//!
//! ```
//! extern crate jsonrpc_core;
//! #[macro_use] extern crate jsonrpc_macros;
//! use jsonrpc_core::{IoHandler, Error};
//! use jsonrpc_core::futures::{self, BoxFuture, Future};
//! build_rpc_trait! {
//! 	pub trait Rpc {
//! 		/// Returns a protocol version
//! 		#[rpc(name = "protocolVersion")]
//! 		fn protocol_version(&self) -> Result<String, Error>;
//!
//! 		/// Adds two numbers and returns a result
//! 		#[rpc(name = "add")]
//! 		fn add(&self, u64, u64) -> Result<u64, Error>;
//!
//! 		/// Performs asynchronous operation
//! 		#[rpc(async, name = "callAsync")]
//! 		fn call(&self, u64) -> BoxFuture<String, Error>;
//! 	}
//! }
//! struct RpcImpl;
//! impl Rpc for RpcImpl {
//! 	fn protocol_version(&self) -> Result<String, Error> {
//! 		Ok("version1".into())
//! 	}
//!
//! 	fn add(&self, a: u64, b: u64) -> Result<u64, Error> {
//! 		Ok(a + b)
//! 	}
//!
//! 	fn call(&self, _: u64) -> BoxFuture<String, Error> {
//! 		futures::finished("OK".to_owned()).boxed()
//! 	}
//! }
//!
//! fn main() {
//!	  let mut io = IoHandler::new();
//!	  let rpc = RpcImpl;
//!
//!	  io.extend_with(rpc.to_delegate());
//! }
//! ```

#![warn(missing_docs)]

pub extern crate jsonrpc_core;
extern crate jsonrpc_pubsub;
extern crate serde;

mod auto_args;
mod delegates;
mod util;

pub mod pubsub;

#[doc(hidden)]
pub use auto_args::{Wrap, WrapAsync, WrapMeta, WrapSubscribe};
pub use auto_args::Trailing;
pub use delegates::IoDelegate;
pub use util::to_value;
