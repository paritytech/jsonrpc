extern crate jsonrpc_core;
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
