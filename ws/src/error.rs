#![allow(missing_docs)]

use std::io;

use ws;

error_chain! {
	foreign_links {
		Io(io::Error);
	}

	errors {
		/// Attempted action on closed connection.
		ConnectionClosed {
			description("connection is closed"),
			display("Action on closed connection."),
		}
	}
}

impl From<ws::Error> for Error {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(e) => e.into(),
			_ => Error::with_chain(err, "WebSockets Error"),
		}
	}
}
