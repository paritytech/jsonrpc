extern crate jsonrpc_core;
extern crate serde;

mod auto_args;
mod delegates;
mod util;

#[doc(hidden)]
pub use auto_args::{Wrap, WrapAsync, WrapMeta};
pub use auto_args::Trailing;
pub use delegates::IoDelegate;
