use std;

/// The result type for this procedural macro.
pub type Result<T> = std::result::Result<T, Error>;
pub type Error = String;

///// Represents errors that may be encountered in
///// invocations of this procedural macro.
//#[derive(Debug)]
//pub struct Error {
//	/// The kind of this error.
//	kind: ErrorKind,
//}
//
///// Kinds of errors that may be encountered in invocations
///// of this procedural macro.
//#[derive(Debug)]
//pub enum ErrorKind {
//	/// Generic error for now, todo: add more specific errors later
//	Generic {
//		message: String
//	}
////	/// When there was an invalid number of arguments passed to `rpc`.
////	InvalidNumberOfArguments {
////		/// The number of found arguments.
////		found: usize,
////	},
//}
//
//impl Error {
//	pub fn generic(msg: &str) -> Self {
//		Error { kind: ErrorKind::Generic { message: msg.to_owned() } }
//	}
//}
