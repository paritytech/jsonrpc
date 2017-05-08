//! JSON-RPC servers utilities.

#![warn(missing_docs)]

#[macro_use]
extern crate log;

extern crate globset;
extern crate jsonrpc_core as core;
pub extern crate tokio_core;
pub extern crate tokio_io;

pub mod cors;
pub mod hosts;
pub mod reactor;
mod matcher;

pub use matcher::Pattern;
