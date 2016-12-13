extern crate futures;
extern crate jsonrpc_core;
extern crate serde;

mod auto_args;
pub mod delegates;
mod util;

#[doc(hidden)]
pub use auto_args::{Wrap, WrapAsync};
pub use auto_args::Trailing;
