//! Ethcore JSON-RPC 2.0 Implementation
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//!
//! use std::sync::Arc;
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//!
//! struct SayHello;
//! impl SyncMethodCommand for SayHello {
//! 	fn execute(&self, _params: Params) -> Result<Value, Error> {
//! 		Ok(Value::String("hello".to_string()))
//! 	}
//! }
//!
//! fn main() {
//! 	let io = IoHandler::new();
//! 	io.add_method("say_hello", SayHello);
//!
//! 	let _server = ServerBuilder::new(Arc::new(io))
//!			.start_http(&"127.0.0.1:3030".parse().unwrap());
//! }
//! ```

/// Transport-agnostic core
extern crate jsonrpc_core;
/// HTTP transport
extern crate jsonrpc_http_server;
/// IPC transport
// extern crate jsonrpc_ipc_server;
/// TCP transport
extern crate jsonrpc_tcp_server;
