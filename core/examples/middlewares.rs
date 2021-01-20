use jsonrpc_core::futures_util::{future::Either, FutureExt};
use jsonrpc_core::*;
use std::future::Future;
use std::sync::atomic::{self, AtomicUsize};
use std::time::Instant;

#[derive(Clone, Debug)]
struct Meta(usize);
impl Metadata for Meta {}

#[derive(Default)]
struct MyMiddleware(AtomicUsize);
impl Middleware<Meta> for MyMiddleware {
	type Future = FutureResponse;
	type CallFuture = middleware::NoopCallFuture;

	fn on_request<F, X>(&self, request: Request, meta: Meta, next: F) -> Either<Self::Future, X>
	where
		F: FnOnce(Request, Meta) -> X + Send,
		X: Future<Output = Option<Response>> + Send + 'static,
	{
		let start = Instant::now();
		let request_number = self.0.fetch_add(1, atomic::Ordering::SeqCst);
		println!("Processing request {}: {:?}, {:?}", request_number, request, meta);

		Either::Left(Box::pin(next(request, meta).map(move |res| {
			println!("Processing took: {:?}", start.elapsed());
			res
		})))
	}
}

pub fn main() {
	let mut io = MetaIoHandler::with_middleware(MyMiddleware::default());

	io.add_method_with_meta("say_hello", |_params: Params, meta: Meta| async move {
		Ok(Value::String(format!("Hello World: {}", meta.0)))
	});

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"Hello World: 5","id":1}"#;

	let headers = 5;
	assert_eq!(
		io.handle_request_sync(request, Meta(headers)),
		Some(response.to_owned())
	);
}
