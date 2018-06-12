// because we reuse the type names as idents in the macros as a dirty hack to
// work around `concat_idents!` being unstable.
#![allow(non_snake_case)]

///! Automatically serialize and deserialize parameters around a strongly-typed function.

use jsonrpc_core::{Error, Params, Value, Metadata, Result};
use jsonrpc_core::futures::{self, Future, IntoFuture};
use jsonrpc_core::futures::future::{self, Either};
use jsonrpc_pubsub::{PubSubMetadata, Subscriber};
use pubsub;
use serde::Serialize;
use serde::de::DeserializeOwned;
use util::{invalid_params, expect_no_params, to_value};

/// Auto-generates an RPC trait from trait definition.
///
/// This just copies out all the methods, docs, and adds another
/// function `to_delegate` which will automatically wrap each strongly-typed
/// function in a wrapper which handles parameter and output type serialization.
///
/// RPC functions may come in a couple forms: synchronous, async and async with metadata.
/// These are parsed with the custom `#[rpc]` attribute, which must follow
/// documentation.
///
/// ## The #[rpc] attribute
///
/// Valid forms:
///  - `#[rpc(name = "name_here")]` (an async rpc function which should be bound to the given name)
///  - `#[rpc(meta, name = "name_here")]` (an async rpc function with metadata which should be bound to the given name)
///
/// Synchronous function format:
/// `fn foo(&self, Param1, Param2, Param3) -> Result<Out>`.
///
/// Asynchronous RPC functions must come in this form:
/// `fn foo(&self, Param1, Param2, Param3) -> BoxFuture<Out>;
///
/// Asynchronous RPC functions with metadata must come in this form:
/// `fn foo(&self, Self::Metadata, Param1, Param2, Param3) -> BoxFuture<Out>;
///
/// Anything else will be rejected by the code generator.
///
/// ## The #[pubsub] attribute
///
/// Valid form:
/// ```rust,ignore
///	#[pubsub(name = "hello")] {
///	  #[rpc(name = "hello_subscribe")]
///	  fn subscribe(&self, Self::Metadata, pubsub::Subscriber<String>, u64);
///	  #[rpc(name = "hello_unsubscribe")]
///	  fn unsubscribe(&self, SubscriptionId) -> Result<bool>;
///	}
///	```
///
/// The attribute is used to create a new pair of subscription methods
/// (if underlying transport supports that.)


#[macro_export]
macro_rules! metadata {
	() => {
		/// Requests metadata
		type Metadata: $crate::jsonrpc_core::Metadata;
	};
	(
		$( $sub_name: ident )+
	) => {
		/// Requests metadata
		type Metadata: $crate::jsonrpc_pubsub::PubSubMetadata;
	};
}

#[macro_export]
macro_rules! build_rpc_trait {
	// entry-point. todo: make another for traits w/ bounds.
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident $(<$($generics:ident),*>)* {
			$(
				$( #[doc=$m_doc:expr] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty $(, $error: ty)* >;
			)*
		}
	) => {
		$(#[$t_attr])*
		pub trait $name $(<$($generics,)*>)* : Sized + Send + Sync + 'static {
			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result<$out $(, $error)* > ;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate<M: $crate::jsonrpc_core::Metadata>(self) -> $crate::IoDelegate<Self, M>
				where $($($generics: Send + Sync + 'static + $crate::Serialize + $crate::DeserializeOwned),*)*
			{
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out $(, $error)* >
					);
				)*
				del
			}
		}
	};

	// entry-point for trait with metadata methods
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident $(<$($generics:ident),*>)* {
			type Metadata;

			$(
				$( #[ doc=$m_doc:expr ] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty $(, $error_std: ty) *>;
			)*

			$(
				#[ pubsub( $($pubsub_t:tt)+ ) ] {
					$( #[ doc= $sub_doc:expr ] )*
					#[ rpc( $($sub_t:tt)* ) ]
					fn $sub_name: ident ( $($sub_p: tt)* );
					$( #[ doc= $unsub_doc:expr ] )*
					#[ rpc( $($unsub_t:tt)* ) ]
					fn $unsub_name: ident ( $($unsub_p: tt)* ) -> $sub_result: tt <$sub_out: ty $(, $error_unsub: ty)* >;
				}
			)*

		}
	) => {
		$(#[$t_attr])*
		pub trait $name $(<$($generics,)*>)* : Sized + Send + Sync + 'static {
			// Metadata bound differs for traits with subscription methods.
			metadata! (
				$( $sub_name )*
			);

			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result <$out $(, $error_std) *>;
			)*

			$(
				$(#[doc=$sub_doc])*
				fn $sub_name ( $($sub_p)* );
				$(#[doc=$unsub_doc])*
				fn $unsub_name ( $($unsub_p)* ) -> $sub_result <$sub_out $(, $error_unsub)* >;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate(self) -> $crate::IoDelegate<Self, Self::Metadata>
				where $($($generics: Send + Sync + 'static + $crate::Serialize + $crate::DeserializeOwned),*)*
			{
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out $(, $error_std)* >
					);
				)*
				$(
					build_rpc_trait!(WRAP del =>
						pubsub: ( $($pubsub_t)* )
						subscribe: ( $($sub_t)* )
						fn $sub_name ( $($sub_p)* );
						unsubscribe: ( $($unsub_t)* )
						fn $unsub_name ( $($unsub_p)* ) -> $sub_result <$sub_out $(, $error_unsub)* >;
					);
				)*
				del
			}
		}
	};

	( WRAP $del: expr =>
		(name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self $(, $param: ty)*) -> $result: tt <$out: ty $(, $error: ty)* >
	) => {
		$del.add_method($name, move |base, params| {
			$crate::WrapAsync::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out $(, $error)*>), base, params)
		});
		$(
			$(
				$del.add_alias($alias, $name);
			)+
		)*
	};

	( WRAP $del: expr =>
		(meta, name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self, Self::Metadata $(, $param: ty)*) -> $result: tt <$out: ty $(, $error: ty)* >
	) => {
		$del.add_method_with_meta($name, move |base, params, meta| {
			$crate::WrapMeta::wrap_rpc(&(Self::$method as fn(&_, Self::Metadata $(, $param)*) -> $result <$out $(, $error)* >), base, params, meta)
		});
		$(
			$(
				$del.add_alias($alias, $name);
			)+
		)*
	};

	( WRAP $del: expr =>
		pubsub: (name = $name: expr)
		subscribe: (name = $subscribe: expr $(, alias = [ $( $sub_alias: expr, )+ ])*)
		fn $sub_method: ident (&self, Self::Metadata $(, $sub_p: ty)+);
		unsubscribe: (name = $unsubscribe: expr $(, alias = [ $( $unsub_alias: expr, )+ ])*)
		fn $unsub_method: ident (&self $(, $unsub_p: ty)+) -> $result: tt <$out: ty $(, $error_unsub: ty)* >;
	) => {
		$del.add_subscription(
			$name,
			($subscribe, move |base, params, meta, subscriber| {
				$crate::WrapSubscribe::wrap_rpc(
					&(Self::$sub_method as fn(&_, Self::Metadata $(, $sub_p)*)),
					base,
					params,
					meta,
					subscriber,
				)
			}),
			($unsubscribe, move |base, id| {
				use $crate::jsonrpc_core::futures::{IntoFuture, Future};
				Self::$unsub_method(base, id).into_future()
					.map($crate::to_value)
					.map_err(Into::into)
			}),
		);

		$(
			$(
				$del.add_alias($sub_alias, $subscribe);
			)*
		)*
		$(
			$(
				$del.add_alias($unsub_alias, $unsubscribe);
			)*
		)*
	};
}

/// A wrapper type without an implementation of `Deserialize`
/// which allows a special implementation of `Wrap` for functions
/// that take a trailing default parameter.
pub struct Trailing<T>(Option<T>);

impl<T> Into<Option<T>> for Trailing<T> {
	fn into(self) -> Option<T> {
		self.0
	}
}

impl<T: DeserializeOwned> Trailing<T> {
	/// Returns a underlying value if present or provided value.
	pub fn unwrap_or(self, other: T) -> T {
		self.0.unwrap_or(other)
	}

	/// Returns an underlying value or computes it if not present.
	pub fn unwrap_or_else<F: FnOnce() -> T>(self, f: F) -> T {
		self.0.unwrap_or_else(f)
	}
}

impl<T: Default + DeserializeOwned> Trailing<T> {
	/// Returns an underlying value or the default value.
	pub fn unwrap_or_default(self) -> T {
		self.0.unwrap_or_default()
	}
}

type WrappedFuture<F, OUT, E> = future::MapErr<
	future::Map<F, fn(OUT) -> Value>,
	fn(E) -> Error
>;
type WrapResult<F, OUT, E> = Either<
	WrappedFuture<F, OUT, E>,
	future::FutureResult<Value, Error>,
>;

fn as_future<F, OUT, E, I>(el: I) -> WrappedFuture<F, OUT, E> where
	OUT: Serialize,
	E: Into<Error>,
	F: Future<Item = OUT, Error = E>,
	I: IntoFuture<Item = OUT, Error = E, Future = F>
{
	el.into_future()
		.map(to_value as fn(OUT) -> Value)
		.map_err(Into::into as fn(E) -> Error)
}

/// Wrapper trait for asynchronous RPC functions.
pub trait WrapAsync<B> {
	/// Output type.
	type Out: IntoFuture<Item = Value, Error = Error>;

	/// Invokes asynchronous RPC method.
	fn wrap_rpc(&self, base: &B, params: Params) -> Self::Out;
}

/// Wrapper trait for meta RPC functions.
pub trait WrapMeta<B, M> {
	/// Output type.
	type Out: IntoFuture<Item = Value, Error = Error>;
	/// Invokes asynchronous RPC method with Metadata.
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> Self::Out;
}

/// Wrapper trait for subscribe RPC functions.
pub trait WrapSubscribe<B, M> {
	/// Invokes subscription.
	fn wrap_rpc(&self, base: &B, params: Params, meta: M, subscriber: Subscriber);
}

// special impl for no parameters.
impl<B, OUT, E, F, I> WrapAsync<B> for fn(&B) -> I where
	B: Send + Sync + 'static,
	OUT: Serialize + 'static,
	E: Into<Error> + 'static,
	F: Future<Item = OUT, Error = E> + Send + 'static,
	I: IntoFuture<Item = OUT, Error = E, Future = F>,
{
	type Out = WrapResult<F, OUT, E>;

	fn wrap_rpc(&self, base: &B, params: Params) -> Self::Out {
		match expect_no_params(params) {
			Ok(()) => Either::A(as_future((self)(base))),
			Err(e) => Either::B(futures::failed(e)),
		}
	}
}

impl<M, B, OUT, E, F, I> WrapMeta<B, M> for fn(&B, M) -> I where
	M: Metadata,
	B: Send + Sync + 'static,
	OUT: Serialize + 'static,
	E: Into<Error> + 'static,
	F: Future<Item = OUT, Error = E> + Send + 'static,
	I: IntoFuture<Item = OUT, Error = E, Future = F>,
{
	type Out = WrapResult<F, OUT, E>;

	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> Self::Out {
		match expect_no_params(params) {
			Ok(()) => Either::A(as_future((self)(base, meta))),
			Err(e) => Either::B(futures::failed(e)),
		}
	}
}

impl<M, B, OUT> WrapSubscribe<B, M> for fn(&B, M, pubsub::Subscriber<OUT>) where
	M: PubSubMetadata,
	B: Send + Sync + 'static,
	OUT: Serialize,
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M, subscriber: Subscriber) {
		match expect_no_params(params) {
			Ok(()) => (self)(base, meta, pubsub::Subscriber::new(subscriber)),
			Err(e) => {
				let _ = subscriber.reject(e);
			},
		}
	}
}

// creates a wrapper implementation which deserializes the parameters,
// calls the function with concrete type, and serializes the output.
macro_rules! wrap {
	($($x: ident),+) => {

		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: DeserializeOwned,)+
			ERR: Into<Error> + 'static,
			X: Future<Item = OUT, Error = ERR> + Send + 'static,
			Z: IntoFuture<Item = OUT, Error = ERR, Future = X>,
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ ) -> Z {
			type Out = WrapResult<X, OUT, ERR>;
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Self::Out {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => Either::A(as_future((self)(base, $($x,)+))),
					Err(e) => Either::B(futures::failed(e)),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: DeserializeOwned,)+
			ERR: Into<Error> + 'static,
			X: Future<Item = OUT, Error = ERR> + Send + 'static,
			Z: IntoFuture<Item = OUT, Error = ERR, Future = X>,
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+) -> Z {
			type Out = WrapResult<X, OUT, ERR>;
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> Self::Out {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => Either::A(as_future((self)(base, meta, $($x,)+))),
					Err(e) => Either::B(futures::failed(e)),
				}
			}
		}

		// subscribe implementation
		impl <
			BASE: Send + Sync + 'static,
			META: PubSubMetadata,
			OUT: Serialize,
			$($x: DeserializeOwned,)+
		> WrapSubscribe<BASE, META> for fn(&BASE, META, pubsub::Subscriber<OUT>, $($x,)+) {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META, subscriber: Subscriber) {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => (self)(base, meta, pubsub::Subscriber::new(subscriber), $($x,)+),
					Err(e) => {
						let _ = subscriber.reject(e);
					},
				}
			}
		}
	}
}

fn params_len(params: &Params) -> Result<usize> {
	match *params {
		Params::Array(ref v) => Ok(v.len()),
		Params::None => Ok(0),
		_ => Err(invalid_params("`params` should be an array", "")),
	}
}

fn require_len(params: &Params, required: usize) -> Result<usize> {
	let len = params_len(params)?;
	if len < required {
		return Err(invalid_params(&format!("`params` should have at least {} argument(s)", required), ""));
	}
	Ok(len)
}

fn parse_trailing_param<T: DeserializeOwned>(params: Params) -> Result<(Option<T>, )> {
	let len = try!(params_len(&params));
	let id = match len {
		0 => Ok((None,)),
		1 => params.parse::<(T,)>().map(|(x, )| (Some(x), )),
		_ => Err(invalid_params("Expecting only one optional parameter.", "")),
	};

	id
}

// special impl for no parameters other than block parameter.
impl<B, OUT, T, E, F, I> WrapAsync<B> for fn(&B, Trailing<T>) -> I where
	B: Send + Sync + 'static,
	OUT: Serialize + 'static,
	T: DeserializeOwned,
	E: Into<Error> + 'static,
	F: Future<Item = OUT, Error = E> + Send + 'static,
	I: IntoFuture<Item = OUT, Error = E, Future = F>,
{
	type Out = WrapResult<F, OUT, E>;
	fn wrap_rpc(&self, base: &B, params: Params) -> Self::Out {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => Either::A(as_future((self)(base, Trailing(id)))),
			Err(e) => Either::B(futures::failed(e)),
		}
	}
}

impl<M, B, OUT, T, E, F, I> WrapMeta<B, M> for fn(&B, M, Trailing<T>) -> I where
	M: Metadata,
	B: Send + Sync + 'static,
	OUT: Serialize + 'static,
	T: DeserializeOwned,
	E: Into<Error> + 'static,
	F: Future<Item = OUT, Error = E> + Send + 'static,
	I: IntoFuture<Item = OUT, Error = E, Future = F>,
{
	type Out = WrapResult<F, OUT, E>;
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> Self::Out {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => Either::A(as_future((self)(base, meta, Trailing(id)))),
			Err(e) => Either::B(futures::failed(e)),
		}
	}
}

impl<M, B, OUT, T> WrapSubscribe<B, M> for fn(&B, M, pubsub::Subscriber<OUT>, Trailing<T>) where
	M: PubSubMetadata,
	B: Send + Sync + 'static,
	OUT: Serialize,
	T: DeserializeOwned,
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M, subscriber: Subscriber) {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => (self)(base, meta, pubsub::Subscriber::new(subscriber), Trailing(id)),
			Err(e) => {
				let _ = subscriber.reject(e);
			},
		}
	}
}

// similar to `wrap!`, but handles a single default trailing parameter
// accepts an additional argument indicating the number of non-trailing parameters.
macro_rules! wrap_with_trailing {
	($num: expr, $($x: ident),+) => {
		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: DeserializeOwned,)+
			TRAILING: DeserializeOwned,
			ERR: Into<Error> + 'static,
			X: Future<Item = OUT, Error = ERR> + Send + 'static,
			Z: IntoFuture<Item = OUT, Error = ERR, Future = X>,
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ Trailing<TRAILING>) -> Z {
			type Out = WrapResult<X, OUT, ERR>;
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Self::Out {
				let len = match require_len(&params, $num) {
					Ok(len) => len,
					Err(e) => return Either::B(futures::failed(e)),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ None)).map_err(Into::into),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ Some(id))).map_err(Into::into),
					_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
				};

				match params {
					Ok(($($x,)+ id)) => Either::A(as_future((self)(base, $($x,)+ Trailing(id)))),
					Err(e) => Either::B(futures::failed(e)),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: DeserializeOwned,)+
			TRAILING: DeserializeOwned,
			ERR: Into<Error> + 'static,
			X: Future<Item = OUT, Error = ERR> + Send + 'static,
			Z: IntoFuture<Item = OUT, Error = ERR, Future = X>,
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+ Trailing<TRAILING>) -> Z {
			type Out = WrapResult<X, OUT, ERR>;
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> Self::Out {
				let len = match require_len(&params, $num) {
					Ok(len) => len,
					Err(e) => return Either::B(futures::failed(e)),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ None)).map_err(Into::into),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ Some(id))).map_err(Into::into),
					_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
				};

				match params {
					Ok(($($x,)+ id)) => Either::A(as_future((self)(base, meta, $($x,)+ Trailing(id)))),
					Err(e) => Either::B(futures::failed(e)),
				}
			}
		}

		// subscribe implementation
		impl <
			BASE: Send + Sync + 'static,
			META: PubSubMetadata,
			OUT: Serialize,
			$($x: DeserializeOwned,)+
			TRAILING: DeserializeOwned,
		> WrapSubscribe<BASE, META> for fn(&BASE, META, pubsub::Subscriber<OUT>, $($x,)+ Trailing<TRAILING>) {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META, subscriber: Subscriber) {
				let len = match require_len(&params, $num) {
					Ok(len) => len,
					Err(e) => {
						let _ = subscriber.reject(e);
						return;
					},
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ None)),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ Some(id))),
					_ => {
						let _ = subscriber.reject(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len)));
						return;
					},
				};

				match params {
					Ok(($($x,)+ id)) => (self)(base, meta, pubsub::Subscriber::new(subscriber), $($x,)+ Trailing(id)),
					Err(e) => {
						let _ = subscriber.reject(e);
						return;
					},
				}
			}
		}
	}
}

wrap!(A, B, C, D, E, F);
wrap!(A, B, C, D, E);
wrap!(A, B, C, D);
wrap!(A, B, C);
wrap!(A, B);
wrap!(A);

wrap_with_trailing!(6, A, B, C, D, E, F);
wrap_with_trailing!(5, A, B, C, D, E);
wrap_with_trailing!(4, A, B, C, D);
wrap_with_trailing!(3, A, B, C);
wrap_with_trailing!(2, A, B);
wrap_with_trailing!(1, A);

