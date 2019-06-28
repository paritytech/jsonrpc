//! High level, typed wrapper for `jsonrpc_core`.
//!
//! Enables creation of "Service" objects grouping a set of RPC methods together in a typed manner.
//!
//! Example
//!
//! ```
//! use jsonrpc_derive::rpc;
//! use jsonrpc_core::{IoHandler, Error, Result};
//! use jsonrpc_core::futures::future::{self, FutureResult};
//!
//! #[rpc]
//! pub trait Rpc {
//! 	#[rpc(name = "protocolVersion")]
//! 	fn protocol_version(&self) -> Result<String>;
//!
//! 	#[rpc(name = "add")]
//! 	fn add(&self, a: u64, b: u64) -> Result<u64>;
//!
//! 	#[rpc(name = "callAsync")]
//! 	fn call(&self, a: u64) -> FutureResult<String, Error>;
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
//!
//! Pub/Sub Example
//!
//! Each subscription must have `subscribe` and `unsubscribe` methods. They can
//! have any name but must be annotated with `subscribe` or `unsubscribe` and
//! have a matching unique subscription name.
//!
//! ```
//! use std::thread;
//! use std::sync::{atomic, Arc, RwLock};
//! use std::collections::HashMap;
//!
//! use jsonrpc_core::{Error, ErrorCode, Result};
//! use jsonrpc_core::futures::Future;
//! use jsonrpc_derive::rpc;
//! use jsonrpc_pubsub::{Session, PubSubHandler, SubscriptionId, typed::{Subscriber, Sink}};
//!
//! #[rpc]
//! pub trait Rpc {
//!		type Metadata;
//!
//!		/// Hello subscription
//!		#[pubsub(
//! 		subscription = "hello",
//! 		subscribe,
//! 		name = "hello_subscribe",
//! 		alias("hello_sub")
//! 	)]
//!		fn subscribe(&self, _: Self::Metadata, _: Subscriber<String>, param: u64);
//!
//!		/// Unsubscribe from hello subscription.
//!		#[pubsub(
//! 		subscription = "hello",
//! 		unsubscribe,
//! 		name = "hello_unsubscribe"
//! 	)]
//!		fn unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;
//! }
//!
//!
//! #[derive(Default)]
//! struct RpcImpl {
//! 	uid: atomic::AtomicUsize,
//! 	active: Arc<RwLock<HashMap<SubscriptionId, Sink<String>>>>,
//! }
//! impl Rpc for RpcImpl {
//! 	type Metadata = Arc<Session>;
//!
//! 	fn subscribe(&self, _meta: Self::Metadata, subscriber: Subscriber<String>, param: u64) {
//! 		if param != 10 {
//! 			subscriber.reject(Error {
//! 				code: ErrorCode::InvalidParams,
//! 				message: "Rejecting subscription - invalid parameters provided.".into(),
//! 				data: None,
//! 			}).unwrap();
//! 			return;
//! 		}
//!
//! 		let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
//! 		let sub_id = SubscriptionId::Number(id as u64);
//! 		let sink = subscriber.assign_id(sub_id.clone()).unwrap();
//! 		self.active.write().unwrap().insert(sub_id, sink);
//! 	}
//!
//! 	fn unsubscribe(&self, _meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
//! 		let removed = self.active.write().unwrap().remove(&id);
//! 		if removed.is_some() {
//! 			Ok(true)
//! 		} else {
//! 			Err(Error {
//! 				code: ErrorCode::InvalidParams,
//! 				message: "Invalid subscription.".into(),
//! 				data: None,
//! 			})
//! 		}
//! 	}
//! }
//!
//! # fn main() {}
//! ```
//!
//! Client Example
//!
//! ```
//! use jsonrpc_core_client::transports::local;
//! use jsonrpc_core::futures::future::{self, Future, FutureResult};
//! use jsonrpc_core::{Error, IoHandler, Result};
//! use jsonrpc_derive::rpc;
//!
//! /// Rpc trait
//! #[rpc]
//! pub trait Rpc {
//! 	/// Returns a protocol version
//! 	#[rpc(name = "protocolVersion")]
//! 	fn protocol_version(&self) -> Result<String>;
//!
//! 	/// Adds two numbers and returns a result
//! 	#[rpc(name = "add", alias("callAsyncMetaAlias"))]
//! 	fn add(&self, a: u64, b: u64) -> Result<u64>;
//!
//! 	/// Performs asynchronous operation
//! 	#[rpc(name = "callAsync")]
//! 	fn call(&self, a: u64) -> FutureResult<String, Error>;
//! }
//!
//! struct RpcImpl;
//!
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
//! 		future::ok("OK".to_owned())
//! 	}
//! }
//!
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.extend_with(RpcImpl.to_delegate());
//!
//! 	let fut = {
//! 		let (client, server) = local::connect::<gen_client::Client, _, _>(io);
//! 		client.add(5, 6).map(|res| println!("5 + 6 = {}", res)).join(server)
//! 	};
//! 	fut.wait().unwrap();
//! }
//!
//! ```

#![recursion_limit = "256"]
#![warn(missing_docs)]

extern crate proc_macro;

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod options;
mod rpc_attr;
mod rpc_trait;
mod to_client;
mod to_delegate;

/// Apply `#[rpc]` to a trait, and a `to_delegate` method is generated which
/// wires up methods decorated with `#[rpc]` or `#[pubsub]` attributes.
/// Attach the delegate to an `IoHandler` and the methods are now callable
/// via JSON-RPC.
#[proc_macro_attribute]
pub fn rpc(args: TokenStream, input: TokenStream) -> TokenStream {
	let input_toks = parse_macro_input!(input as syn::Item);

	let options = match options::DeriveOptions::try_from(args) {
		Ok(options) => options,
		Err(error) => return error.to_compile_error().into(),
	};

	match rpc_trait::rpc_impl(input_toks, options) {
		Ok(output) => output.into(),
		Err(err) => err.to_compile_error().into(),
	}
}
