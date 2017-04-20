// because we reuse the type names as idents in the macros as a dirty hack to
// work around `concat_idents!` being unstable.
#![allow(non_snake_case)]

///! Automatically serialize and deserialize parameters around a strongly-typed function.

use jsonrpc_core::{Error, Params, Value, Metadata};
use jsonrpc_core::futures::{self, BoxFuture, Future};
use jsonrpc_pubsub::{PubSubMetadata, Subscriber};
use pubsub;
use serde::{Serialize, Deserialize};
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
///  - `#[rpc(name = "name_here")]` (a synchronous rpc function which should be bound to the given name)
///  - `#[rpc(async, name = "name_here")]` (an async rpc function which should be bound to the given name)
///  - `#[rpc(meta, name = "name_here")]` (an async rpc function with metadata which should be bound to the given name)
///
/// Synchronous function format:
/// `fn foo(&self, Param1, Param2, Param3) -> Result<Out, Error>`.
///
/// Asynchronous RPC functions must come in this form:
/// `fn foo(&self, Param1, Param2, Param3) -> BoxFuture<Out, Error>;
///
/// Asynchronous RPC functions with metadata must come in this form:
/// `fn foo(&self, Self::Metadata, Param1, Param2, Param3) -> BoxFuture<Out, Error>;
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
///	  fn unsubscribe(&self, SubscriptionId) -> BoxFuture<bool, Error>;
///	}
///	```
///
/// The attribute is used to create a new pair of subscription methods
/// (if underlying transport supports that.)


#[macro_export]
macro_rules! metadata {
	() => {
		/// Requests metadata
		type Metadata: ::jsonrpc_core::Metadata;
	};
	(
		$( $sub_name: ident )+
	) => {
		/// Requests metadata
		type Metadata: ::jsonrpc_pubsub::PubSubMetadata;
	};
}

#[macro_export]
macro_rules! build_rpc_trait {
	// entry-point. todo: make another for traits w/ bounds.
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident {
			$(
				$( #[doc=$m_doc:expr] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty, Error>;
			)*
		}
	) => {
		$(#[$t_attr])*
		pub trait $name: Sized + Send + Sync + 'static {
			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result<$out, Error> ;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate<M: ::jsonrpc_core::Metadata>(self) -> $crate::IoDelegate<Self, M> {
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out, Error>
					);
				)*
				del
			}
		}
	};

	// entry-point for trait with metadata methods
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident {
			type Metadata;

			$(
				$( #[ doc=$m_doc:expr ] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty, Error>;
			)*

			$(
				#[ pubsub( $($pubsub_t:tt)+ ) ] {
					$( #[ doc= $sub_doc:expr ] )*
					#[ rpc( $($sub_t:tt)* ) ]
					fn $sub_name: ident ( $($sub_p: tt)* );
					$( #[ doc= $unsub_doc:expr ] )*
					#[ rpc( $($unsub_t:tt)* ) ]
					fn $unsub_name: ident ( $($unsub_p: tt)* ) -> $sub_result: tt <$sub_out: ty, Error>;
				}
			)*

		}
	) => {
		$(#[$t_attr])*
		pub trait $name: Sized + Send + Sync + 'static {
			// Metadata bound differs for traits with subscription methods.
			metadata! (
				$( $sub_name )*
			);

			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result <$out, Error>;
			)*

			$(
				$(#[doc=$sub_doc])*
				fn $sub_name ( $($sub_p)* );
				$(#[doc=$unsub_doc])*
				fn $unsub_name ( $($unsub_p)* ) -> $sub_result <$sub_out, Error>;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate(self) -> $crate::IoDelegate<Self, Self::Metadata> {
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out, Error>
					);
				)*
				$(
					build_rpc_trait!(WRAP del =>
						pubsub: ( $($pubsub_t)* )
						subscribe: ( $($sub_t)* )
						fn $sub_name ( $($sub_p)* );
						unsubscribe: ( $($unsub_t)* )
						fn $unsub_name ( $($unsub_p)* ) -> $sub_result <$sub_out, Error>;
					);
				)*
				del
			}
		}
	};

	( WRAP $del: expr =>
		(name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_method($name, move |base, params| {
			$crate::Wrap::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out, Error>), base, params)
		});
		$(
			$(
				$del.add_alias($alias, $name);
			)+
		)*
	};

	( WRAP $del: expr =>
		(async, name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_async_method($name, move |base, params| {
			$crate::WrapAsync::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out, Error>), base, params)
		});
		$(
			$(
				$del.add_alias($alias, $name);
			)+
		)*
	};

	( WRAP $del: expr =>
		(meta, name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self, Self::Metadata $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_method_with_meta($name, move |base, params, meta| {
			$crate::WrapMeta::wrap_rpc(&(Self::$method as fn(&_, Self::Metadata $(, $param)*) -> $result <$out, Error>), base, params, meta)
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
		fn $unsub_method: ident (&self $(, $unsub_p: ty)+) -> $result: tt <$out: ty, Error>;
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
				use $crate::jsonrpc_core::futures::Future;
				Self::$unsub_method(base, id).map($crate::to_value).boxed()
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
pub struct Trailing<T: Default + Deserialize>(pub T);

/// Wrapper trait for synchronous RPC functions.
pub trait Wrap<B> {
	/// Invokes RPC method.
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error>;
}

/// Wrapper trait for asynchronous RPC functions.
pub trait WrapAsync<B> {
	/// Invokes asynchronous RPC method.
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error>;
}

/// Wrapper trait for meta RPC functions.
pub trait WrapMeta<B, M> {
	/// Invokes asynchronous RPC method with Metadata.
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error>;
}

/// Wrapper trait for subscribe RPC functions.
pub trait WrapSubscribe<B, M> {
	/// Invokes subscription.
	fn wrap_rpc(&self, base: &B, params: Params, meta: M, subscriber: Subscriber);
}

// special impl for no parameters.
impl<B, OUT> Wrap<B> for fn(&B) -> Result<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static
{
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error> {
		expect_no_params(params)
			.and_then(|()| (self)(base))
			.map(to_value)
	}
}

impl<B, OUT> WrapAsync<B> for fn(&B) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static
{
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error> {
		match expect_no_params(params) {
			Ok(()) => (self)(base).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

impl<B, M, OUT> WrapMeta<B, M> for fn(&B, M) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, M: Metadata
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error> {
		match expect_no_params(params) {
			Ok(()) => (self)(base, meta).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

impl<B, M, OUT> WrapSubscribe<B, M> for fn(&B, M, pubsub::Subscriber<OUT>)
	where B: Send + Sync + 'static, OUT: Serialize, M: PubSubMetadata
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

		// synchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> Wrap<BASE> for fn(&BASE, $($x,)+) -> Result<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Result<Value, Error> {
				params.parse::<($($x,)+)>().and_then(|($($x,)+)| {
					(self)(base, $($x,)+)
				}).map(to_value)
			}
		}

		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ ) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> BoxFuture<Value, Error> {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => (self)(base, $($x,)+).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> BoxFuture<Value, Error> {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => (self)(base, meta, $($x,)+).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}

		// subscribe implementation
		impl <
			BASE: Send + Sync + 'static,
			META: PubSubMetadata,
			OUT: Serialize,
			$($x: Deserialize,)+
		> WrapSubscribe<BASE, META> for fn(&BASE, META, pubsub::Subscriber<OUT>, $($x,)+) {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META, subscriber: Subscriber){
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

fn params_len(params: &Params) -> Result<usize, Error> {
	match *params {
		Params::Array(ref v) => Ok(v.len()),
		Params::None => Ok(0),
		_ => Err(invalid_params("`params` should be an array", "")),
	}
}

fn parse_trailing_param<T: Default + Deserialize>(params: Params) -> Result<(T, ), Error> {
	let len = try!(params_len(&params));
	let id = match len {
		0 => Ok((T::default(),)),
		1 => params.parse::<(T,)>(),
		_ => Err(invalid_params("Expecting only one optional parameter.", "")),
	};

	id
}

// special impl for no parameters other than block parameter.
impl<B, OUT, T> Wrap<B> for fn(&B, Trailing<T>) -> Result<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize
{
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error> {
		let id = try!(parse_trailing_param(params)).0;

		(self)(base, Trailing(id)).map(to_value)
	}
}

impl<B, OUT, T> WrapAsync<B> for fn(&B, Trailing<T>) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize
{
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error> {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => (self)(base, Trailing(id)).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

impl<B, M, OUT, T> WrapMeta<B, M> for fn(&B, M, Trailing<T>) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize, M: Metadata,
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error> {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => (self)(base, meta, Trailing(id)).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

// similar to `wrap!`, but handles a single default trailing parameter
// accepts an additional argument indicating the number of non-trailing parameters.
macro_rules! wrap_with_trailing {
	($num: expr, $($x: ident),+) => {
		// synchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> Wrap<BASE> for fn(&BASE, $($x,)+ Trailing<TRAILING>) -> Result<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Result<Value, Error> {
				let len = try!(params_len(&params));

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
				};

				let ($($x,)+ id) = try!(params);
				(self)(base, $($x,)+ Trailing(id)).map(to_value)
			}
		}

		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ Trailing<TRAILING>) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> BoxFuture<Value, Error> {
				let len = match params_len(&params) {
					Ok(len) => len,
					Err(e) => return futures::failed(e).boxed(),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
				};

				match params {
					Ok(($($x,)+ id)) => (self)(base, $($x,)+ Trailing(id)).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+ Trailing<TRAILING>) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> BoxFuture<Value, Error> {
				let len = match params_len(&params) {
					Ok(len) => len,
					Err(e) => return futures::failed(e).boxed(),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
				};

				match params {
					Ok(($($x,)+ id)) => (self)(base, meta, $($x,)+ Trailing(id)).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}
	}
}

wrap!(A, B, C, D, E);
wrap!(A, B, C, D);
wrap!(A, B, C);
wrap!(A, B);
wrap!(A);

wrap_with_trailing!(5, A, B, C, D, E);
wrap_with_trailing!(4, A, B, C, D);
wrap_with_trailing!(3, A, B, C);
wrap_with_trailing!(2, A, B);
wrap_with_trailing!(1, A);

