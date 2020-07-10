//! `IoHandler` middlewares

use crate::calls::Metadata;
use crate::types::{Call, Output, Request, Response};
use futures::{future::Either, Future};
use std::pin::Pin;

/// RPC middleware
pub trait Middleware<M: Metadata>: Send + Sync + 'static {
	/// A returned request future.
	type Future: Future<Output = Option<Response>> + Send + 'static;

	/// A returned call future.
	type CallFuture: Future<Output = Option<Output>> + Send + 'static;

	/// Method invoked on each request.
	/// Allows you to either respond directly (without executing RPC call)
	/// or do any additional work before and/or after processing the request.
	fn on_request<F, X>(&self, request: Request, meta: M, next: F) -> Either<Self::Future, X>
	where
		F: Fn(Request, M) -> X + Send + Sync,
		X: Future<Output = Option<Response>> + Send + 'static,
	{
		Either::Right(next(request, meta))
	}

	/// Method invoked on each call inside a request.
	///
	/// Allows you to either handle the call directly (without executing RPC call).
	fn on_call<F, X>(&self, call: Call, meta: M, next: F) -> Either<Self::CallFuture, X>
	where
		F: Fn(Call, M) -> X + Send + Sync,
		X: Future<Output = Option<Output>> + Send + 'static,
	{
		Either::Right(next(call, meta))
	}
}

/// Dummy future used as a noop result of middleware.
pub type NoopFuture = Pin<Box<dyn Future<Output = Option<Response>> + Send>>;
/// Dummy future used as a noop call result of middleware.
pub type NoopCallFuture = Pin<Box<dyn Future<Output = Option<Output>> + Send>>;

/// No-op middleware implementation
#[derive(Clone, Debug, Default)]
pub struct Noop;
impl<M: Metadata> Middleware<M> for Noop {
	type Future = NoopFuture;
	type CallFuture = NoopCallFuture;
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>> Middleware<M> for (A, B) {
	type Future = Either<A::Future, B::Future>;
	type CallFuture = Either<A::CallFuture, B::CallFuture>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X>
	where
		F: Fn(Request, M) -> X + Send + Sync,
		X: Future<Output = Option<Response>> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, |request, meta| {
			self.1.on_request(request, meta, &process)
		}))
	}

	fn on_call<F, X>(&self, call: Call, meta: M, process: F) -> Either<Self::CallFuture, X>
	where
		F: Fn(Call, M) -> X + Send + Sync,
		X: Future<Output = Option<Output>> + Send + 'static,
	{
		repack(
			self.0
				.on_call(call, meta, |call, meta| self.1.on_call(call, meta, &process)),
		)
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>> Middleware<M> for (A, B, C) {
	type Future = Either<A::Future, Either<B::Future, C::Future>>;
	type CallFuture = Either<A::CallFuture, Either<B::CallFuture, C::CallFuture>>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X>
	where
		F: Fn(Request, M) -> X + Send + Sync,
		X: Future<Output = Option<Response>> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, |request, meta| {
			repack(self.1.on_request(request, meta, |request, meta| {
				self.2.on_request(request, meta, &process)
			}))
		}))
	}

	fn on_call<F, X>(&self, call: Call, meta: M, process: F) -> Either<Self::CallFuture, X>
	where
		F: Fn(Call, M) -> X + Send + Sync,
		X: Future<Output = Option<Output>> + Send + 'static,
	{
		repack(self.0.on_call(call, meta, |call, meta| {
			repack(
				self.1
					.on_call(call, meta, |call, meta| self.2.on_call(call, meta, &process)),
			)
		}))
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>, D: Middleware<M>> Middleware<M>
	for (A, B, C, D)
{
	type Future = Either<A::Future, Either<B::Future, Either<C::Future, D::Future>>>;
	type CallFuture = Either<A::CallFuture, Either<B::CallFuture, Either<C::CallFuture, D::CallFuture>>>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X>
	where
		F: Fn(Request, M) -> X + Send + Sync,
		X: Future<Output = Option<Response>> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, |request, meta| {
			repack(self.1.on_request(request, meta, |request, meta| {
				repack(self.2.on_request(request, meta, |request, meta| {
					self.3.on_request(request, meta, &process)
				}))
			}))
		}))
	}

	fn on_call<F, X>(&self, call: Call, meta: M, process: F) -> Either<Self::CallFuture, X>
	where
		F: Fn(Call, M) -> X + Send + Sync,
		X: Future<Output = Option<Output>> + Send + 'static,
	{
		repack(self.0.on_call(call, meta, |call, meta| {
			repack(self.1.on_call(call, meta, |call, meta| {
				repack(
					self.2
						.on_call(call, meta, |call, meta| self.3.on_call(call, meta, &process)),
				)
			}))
		}))
	}
}

#[inline(always)]
fn repack<A, B, X>(result: Either<A, Either<B, X>>) -> Either<Either<A, B>, X> {
	match result {
		Either::Left(a) => Either::Left(Either::Left(a)),
		Either::Right(Either::Left(b)) => Either::Left(Either::Right(b)),
		Either::Right(Either::Right(x)) => Either::Right(x),
	}
}
