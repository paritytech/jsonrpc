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

		/// WebSockets error.
		WebSocket(t: ws::Error) {
			description("WebSockets error"),
			display("WebSocket: {}", t),
		}
	}
}

/// Custom `From<ws::Error>` implementation to unpack `io::Error`.
impl From<ws::Error> for Error {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(e) => e.into(),
			_ => ErrorKind::WebSocket(err).into(),
		}
	}
}

