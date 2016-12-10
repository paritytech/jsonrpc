extern crate serde;
extern crate jsonrpc_core;

mod auto_args;
mod util;

#[doc(hidden)]
pub use auto_args::{Wrap, WrapAsync};
