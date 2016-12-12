//! Ethcore JSON-RPC 2.0 Implementation
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_http_server;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_http_server::*;
//!
//! fn main() {
//! 	let mut io = IoHandler::default();
//! 	io.add_method("say_hello", |_params| {
//!			Ok(Value::String("hello".into()))
//! 	});
//!
//! 	let _server = ServerBuilder::new(io)
//!			.start_http(&"127.0.0.1:3030".parse().unwrap());
//! }
//! ```

/// Transport-agnostic core
extern crate jsonrpc_core;
/// HTTP transport
extern crate jsonrpc_http_server;
/// IPC transport
extern crate jsonrpc_ipc_server;
/// TCP transport
extern crate jsonrpc_tcp_server;
