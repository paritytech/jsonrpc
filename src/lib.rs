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
extern crate slab;
extern crate miow;
#[cfg(test)]
extern crate rand;

#[cfg(not(windows))]
mod nix;

#[cfg(windows)]
mod win;

#[cfg(test)]
pub mod tests;

#[cfg(not(windows))]
pub use nix::{Server, Error};

#[cfg(windows)]
pub use win::{Server, Error, Result as PipeResult};

