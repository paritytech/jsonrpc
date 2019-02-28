#![allow(missing_docs)]

use std::{io, error, fmt, result};

use crate::ws;

#[derive(Debug)]
pub enum Error {
	Io(io::Error),
	WsError(ws::Error),
	ConnectionClosed,
}

pub type Result<T> = result::Result<T, Error>;

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
		match self {
			Error::ConnectionClosed => write!(f, "Action on closed connection."),
			Error::WsError(err) => write!(f, "WebSockets Error: {}", err),
			Error::Io(err) => write!(f, "Io Error: {}", err),
		}
	}
}

impl error::Error for Error {
	fn source(&self) -> Option<&(error::Error + 'static)> {
		match self {
			Error::Io(io) => Some(io),
			Error::WsError(ws) => Some(ws),
			Error::ConnectionClosed => None,
		}
	}
}

impl From<io::Error> for Error {
	fn from(err: io::Error) -> Self {
		Error::Io(err)
	}
}

impl From<ws::Error> for Error {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(err) => Error::Io(err),
			_ => Error::WsError(err),
		}
	}
}
