#![allow(missing_docs)]

use std::io;

use hyper;

error_chain! {
	foreign_links {
		Io(io::Error);
	}

	errors {
		/// Hyper error
		Hyper(e: hyper::Error) {
			description("hyper error"),
			display("Hyper: {}", e),
		}
	}
}

impl From<hyper::Error> for Error {
	fn from(err: hyper::Error) -> Self {
		match err {
			hyper::Error::Io(e) => e.into(),
			e => ErrorKind::Hyper(e).into(),
		}
	}
}
