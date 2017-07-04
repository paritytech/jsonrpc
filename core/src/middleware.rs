//! `IoHandler` middlewares

use io::FutureResponse;
use calls::Metadata;
use types::{Request, Response};
use futures::Future;

/// RPC middleware
pub trait Middleware<M: Metadata>: Send + Sync + 'static {
	/// Method invoked on each request.
	/// Allows you to either respond directly (without executing RPC call)
	/// or do any additional work before and/or after processing the request.
	fn on_request<F, X>(&self, request: Request, meta: M, next: F) -> FutureResponse where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static;
}

/// No-op middleware implementation
#[derive(Default)]
pub struct Noop;
impl<M: Metadata> Middleware<M> for Noop {
	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		process(request, meta).boxed()
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>>
	Middleware<M> for (A, B)
{
	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		self.0.on_request(request, meta, move |request, meta| {
			self.1.on_request(request, meta, process)
		})
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>>
	Middleware<M> for (A, B, C)
{
	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		self.0.on_request(request, meta, move |request, meta| {
			self.1.on_request(request, meta, move |request, meta| {
				self.2.on_request(request, meta, process)
			})
		})
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>, D: Middleware<M>>
	Middleware<M> for (A, B, C, D)
{
	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		self.0.on_request(request, meta, move |request, meta| {
			self.1.on_request(request, meta, move |request, meta| {
				self.2.on_request(request, meta, move |request, meta| {
					self.3.on_request(request, meta, process)
				})
			})
		})
	}
}
