//! `IoHandler` middlewares

use io::FutureResponse;
use calls::Metadata;
use types::Request;

/// RPC middleware
pub trait Middleware<M: Metadata>: Send + Sync + 'static {
	/// Method invoked on each request.
	/// Allows you to either respond directly (without executing RPC call)
	/// or do any additional work before and/or after processing the request.
	fn on_request<F>(&self, request: Request, meta: M, next: F) -> FutureResponse where
		F: FnOnce(Request, M) -> FutureResponse;
}

/// No-op middleware implementation
#[derive(Default)]
pub struct Noop;
impl<M: Metadata> Middleware<M> for Noop {
	fn on_request<F>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> FutureResponse,
	{
		process(request, meta)
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>>
	Middleware<M> for (A, B)
{
	fn on_request<F>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> FutureResponse,
	{
		self.0.on_request(request, meta, move |request, meta| {
			self.1.on_request(request, meta, process)
		})
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>>
	Middleware<M> for (A, B, C)
{
	fn on_request<F>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> FutureResponse,
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
	fn on_request<F>(&self, request: Request, meta: M, process: F) -> FutureResponse where
		F: FnOnce(Request, M) -> FutureResponse,
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
