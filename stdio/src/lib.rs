//! jsonrpc server using stdin/stdout
//!
//! ```no_run
//!
//! use jsonrpc_stdio_server::ServerBuilder;
//! use jsonrpc_stdio_server::jsonrpc_core::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut io = IoHandler::default();
//!     io.add_sync_method("say_hello", |_params| {
//!         Ok(Value::String("hello".to_owned()))
//!     });
//!
//!     let server = ServerBuilder::new(io).build();
//!     server.await;
//! }
//! ```

#![deny(missing_docs)]

use std::future::Future;
use std::sync::Arc;

#[macro_use]
extern crate log;

pub use jsonrpc_core;
pub use tokio;

use jsonrpc_core::{MetaIoHandler, Metadata, Middleware};
use tokio_util::codec::{FramedRead, LinesCodec};

/// Stdio server builder
pub struct ServerBuilder<M: Metadata = (), T: Middleware<M> = jsonrpc_core::NoopMiddleware> {
	handler: Arc<MetaIoHandler<M, T>>,
}

impl<M: Metadata, T: Middleware<M>> ServerBuilder<M, T>
where
	M: Default,
	T::Future: Unpin,
	T::CallFuture: Unpin,
{
	/// Returns a new server instance
	pub fn new(handler: impl Into<MetaIoHandler<M, T>>) -> Self {
		ServerBuilder {
			handler: Arc::new(handler.into()),
		}
	}

	/// Returns a server future that needs to be polled in order to make progress.
	///
	/// Will block until EOF is read or until an error occurs.
	/// The server reads from STDIN line-by-line, one request is taken
	/// per line and each response is written to STDOUT on a new line.
	pub fn build(&self) -> impl Future<Output = ()> + 'static {
		let handler = self.handler.clone();

		async move {
			let stdin = tokio::io::stdin();
			let mut stdout = tokio::io::stdout();

			let mut framed_stdin = FramedRead::new(stdin, LinesCodec::new());

			use futures::StreamExt;
			while let Some(request) = framed_stdin.next().await {
				match request {
					Ok(line) => {
						let res = Self::process(&handler, line).await;
						let mut sanitized = res.replace('\n', "");
						sanitized.push('\n');
						use tokio::io::AsyncWriteExt;
						if let Err(e) = stdout.write_all(sanitized.as_bytes()).await {
							log::warn!("Error writing response: {:?}", e);
						}
					}
					Err(e) => {
						log::warn!("Error reading line: {:?}", e);
					}
				}
			}
		}
	}

	/// Process a request asynchronously
	fn process(io: &Arc<MetaIoHandler<M, T>>, input: String) -> impl Future<Output = String> + Send {
		use jsonrpc_core::futures::FutureExt;
		let f = io.handle_request(&input, Default::default());
		f.map(move |result| match result {
			Some(res) => res,
			None => {
				info!("JSON RPC request produced no response: {:?}", input);
				String::from("")
			}
		})
	}
}
