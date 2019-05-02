//! Client transport implementations
use failure::format_err;
use futures::{sync::mpsc, Future, Stream, Sink};
//use crate::{RpcError, RpcClient, RpcChannel};

pub mod local;
pub mod http;
pub mod duplex;

pub use duplex::Duplex;
