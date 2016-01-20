//! Unsafe serde deserialize extension
use std::{ptr, mem};
use serde::de::{Deserialize, Deserializer};

pub trait Peek: Sized {
	/// Peek at next deserialized value and consume it only if it's Ok.
	fn peek<D>(deserializer: &mut D) -> Result<Self, D::Error> where D: Deserializer;
}

/// Looking at Serde library, it seems that we can do this and it wont leak.
impl<T> Peek for T where T: Deserialize {
	fn peek<D>(deserializer: &mut D) -> Result<Self, D::Error> where D: Deserializer {
		unsafe {
			let mut d: D = mem::uninitialized();
			ptr::copy(deserializer, &mut d, 1);
			let res = T::deserialize(&mut d);
			if res.is_ok() {
				mem::swap(deserializer, &mut d);
			}
			mem::forget(d);
			res
		}
	}
}
