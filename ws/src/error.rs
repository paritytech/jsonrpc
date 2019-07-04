use std::{error, fmt, io, result};

use crate::ws;

/// WebSockets Server Error
#[derive(Debug)]
pub enum Error {
	/// Io Error
	Io(io::Error),
	/// WebSockets Error
	WsError(ws::Error),
	/// Connection Closed
	ConnectionClosed,
}

/// WebSockets Server Result
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
	fn source(&self) -> Option<&(dyn error::Error + 'static)> {
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
