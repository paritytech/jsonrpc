// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

extern crate jsonrpc_core;
extern crate jsonrpc_server_utils;
extern crate rand;

#[macro_use] extern crate log;

#[cfg(test)] #[macro_use] extern crate lazy_static;
#[cfg(test)] extern crate env_logger;
#[cfg(test)] mod logger;

#[cfg(windows)]
extern crate miow;

#[cfg(windows)]
mod validator;

#[cfg(test)]
#[cfg(windows)]
mod tests;

#[cfg(windows)] mod win;
#[cfg(windows)] pub use win::{Server, Error, Result as PipeResult, server};

#[cfg(not(windows))] mod stream_codec;
#[cfg(not(windows))] mod uds;
#[cfg(not(windows))] mod meta;
#[cfg(not(windows))] pub use uds::{server, Server, ServerBuilder};

pub use self::jsonrpc_server_utils::tokio_core;
pub use self::jsonrpc_server_utils::reactor::UninitializedRemote;
