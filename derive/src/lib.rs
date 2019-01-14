//! High level, typed wrapper for `jsonrpc_core`.
//!
//! Enables creation of "Service" objects grouping a set of RPC methods together in a typed manner.
//!
//! Example
//!
//! ```
//! #[macro_use]
//! extern crate jsonrpc_derive;
//! extern crate jsonrpc_core;
//! use jsonrpc_core::{IoHandler, Error, Result};
//! use jsonrpc_core::futures::future::{self, FutureResult};
//!
//! #[rpc]
//! pub trait Rpc {
//! 	#[rpc(name = "protocolVersion")]
//! 	fn protocol_version(&self) -> Result<String>;
//!
//! 	#[rpc(name = "add")]
//! 	fn add(&self, _: u64, _: u64) -> Result<u64>;
//!
//! 	#[rpc(name = "callAsync")]
//! 	fn call(&self, _: u64) -> FutureResult<String, Error>;
//! }
//!
//! struct RpcImpl;
//! impl Rpc for RpcImpl {
//! 	fn protocol_version(&self) -> Result<String> {
//! 		Ok("version1".into())
//! 	}
//!
//! 	fn add(&self, a: u64, b: u64) -> Result<u64> {
//! 		Ok(a + b)
//! 	}
//!
//! 	fn call(&self, _: u64) -> FutureResult<String, Error> {
//! 		future::ok("OK".to_owned()).into()
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

#![recursion_limit = "256"]

extern crate proc_macro;

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod rpc_attr;
mod rpc_trait;
mod to_delegate;

// todo: [AJ] docs
#[proc_macro_attribute]
pub fn rpc(args: TokenStream, input: TokenStream) -> TokenStream {
	let args_toks = parse_macro_input!(args as syn::AttributeArgs);
	let input_toks = parse_macro_input!(input as syn::Item);

	let output = match rpc_trait::rpc_impl(args_toks, input_toks) {
		Ok(output) => output,
		Err(err) => panic!("[rpc] encountered error: {}", err),
	};

	output.into()
}
