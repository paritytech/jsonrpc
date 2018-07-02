//! jsonrpc typed params
//!
//! Structs in this module are responsible to partially deserialize params of the request
//! Ideally without copying.

/// Deserialize only one parameter under particular index in the parameters array.
pub mod indexed {
	use std::{
		fmt,
		ops::Deref,
	};
	use serde::de;

	pub trait Index {
		fn index() -> usize;
	}

	mod index {
		use super::Index;

		pub struct First;
		impl Index for First {
			fn index() -> usize { 0 }
		}

		pub struct Second;
		impl Index for Second {
			fn index() -> usize { 1 }
		}

		pub struct Third;
		impl Index for Third {
			fn index() -> usize { 2 }
		}

		pub struct Fourth;
		impl Index for Fourth {
			fn index() -> usize { 3 }
		}

		pub struct Fifth;
		impl Index for Fifth {
			fn index() -> usize { 4 }
		}
	}

	pub struct Param<I, T> {
		index: I,
		value: T,
	}

	impl<I, T> Param<I, T> {
		pub fn into_inner(self) -> T {
			self.value
		}
	}

	impl<I, T> Deref for Param<I, T> {
		type Target = T;

		fn deref(&self) -> &Self::Target {
			&self.value
		}
	}

	impl<'a, I, T> de::Deserialize<'a> for Param<I, T> where
		I: Index,
		T: de::Deserialize<'a>,
	{
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where D: de::Deserializer<'a> {
			deserializer.deserialize_seq(ParamVisitor {
				_d: Default::default(),
			})
		}
	}

	struct ParamVisitor<I, T> {
		_d: ::std::marker::PhantomData<(I, T)>,
	}

	impl<'a, I, T> de::Visitor<'a> for ParamVisitor<I, T> where
		I: Index,
		T: de::Deserialize<'a>,
	{
		type Value = Param<I, T>;

		fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
			write!(fmt, "a list of at least {} elements", I::index())
		}

		fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where
			A: de::SeqAccess<'a>,
		{
			for i in 0..I::index() {
				if let None = seq.next_element::<de::IgnoredAny>()? {
					return Err(de::Error::invalid_length(i, &self))
				}
			}

			let nth = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(I::index(), &self))?;

			// consume the rest
			while seq.next_element::<de::IgnoredAny>()?.is_some() {}

			Ok(nth)
		}
	}
}

pub mod keyed {
	// TODO [ToDr] expose a macro for indexed and keyed
	pub struct KeyedParam;
}

#[cfg(test)]
mod tests {

	#[test]
	fn should_have_tests() {
		assert_eq!(true, false)
	}
}
