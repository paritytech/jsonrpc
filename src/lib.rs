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
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate env_logger;

#[cfg(windows)]
extern crate miow;

#[cfg(not(windows))]
extern crate slab;
#[cfg(not(windows))]
extern crate mio;
#[cfg(not(windows))]
extern crate bytes;

#[cfg(test)]
extern crate rand;

#[cfg(not(windows))] mod nix;
#[cfg(windows)] mod win;

mod validator;

#[cfg(test)] pub mod tests;

#[cfg(not(windows))] pub use nix::{Server, Error};

#[cfg(windows)] pub use win::{Server, Error, Result as PipeResult};

use std::env;
use log::LogLevelFilter;
use env_logger::LogBuilder;

lazy_static! {
	static ref LOG_DUMMY: bool = {
		let mut builder = LogBuilder::new();
		builder.filter(None, LogLevelFilter::Info);

		if let Ok(log) = env::var("RUST_LOG") {
			builder.parse(&log);
		}

		if let Ok(_) = builder.init() {
			println!("logger initialized");
		}
		true
	};
}

/// Intialize log with default settings
pub fn init_log() {
	let _ = *LOG_DUMMY;
}
