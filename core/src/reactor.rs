//! Implementation of transport-agnostic Core Event Loop.

extern crate tokio_core;

use std::thread;
use std::sync::{mpsc, Arc};

use futures::{self, Future};
use self::tokio_core::reactor::Remote;

use {MetaIoHandler, Metadata};

/// EventLoop for all request to `jsonrpc-core`.
/// NOTE: This is more-less temporary solution until we find a good way of integrating with event loops of particular
/// transports.
pub struct RpcEventLoop {
	remote: Remote,
	handle: RpcEventLoopHandle,
}

impl RpcEventLoop {
	/// Spawns a new thread with `EventLoop` with given handler.
	pub fn spawn() -> Self {
		let (stop, stopped) = futures::oneshot();
		let (tx, rx) = mpsc::channel();
		let handle = thread::spawn(move || {
			let mut el = tokio_core::reactor::Core::new().expect("Creating an event loop should not fail.");
			tx.send(el.remote()).expect("Rx is blocking upper thread.");
			let _ = el.run(futures::empty().select(stopped));
		});
		let remote = rx.recv().expect("tx is transfered to a newly spawned thread.");

		RpcEventLoop {
			remote: remote,
			handle: RpcEventLoopHandle {
				close: Some(stop),
				handle: Some(handle),
			},
		}
	}

	/// Returns an RPC handler to process requests.
	pub fn handler<M: Metadata>(&self, handler: Arc<MetaIoHandler<M>>) -> RpcHandler<M> {
		RpcHandler::new(handler, self.remote.clone())
	}

	/// Returns event loop remote.
	pub fn remote(&self) -> Remote {
		self.remote.clone()
	}
}

/// A handle to running event loop. Dropping the handle will cause event loop to finish.
pub struct RpcEventLoopHandle {
	close: Option<futures::Complete<()>>,
	handle: Option<thread::JoinHandle<()>>
}

impl From<RpcEventLoop> for RpcEventLoopHandle {
	fn from(el: RpcEventLoop) -> Self {
		el.handle
	}
}

impl Drop for RpcEventLoopHandle {
	fn drop(&mut self) {
		self.close.take().map(|v| v.complete(()));
	}
}

impl RpcEventLoopHandle {
	/// Blocks current thread and waits until the event loop is finished.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}

	/// Finishes this event loop.
	pub fn close(mut self) {
		self.close.take().unwrap().complete(())
	}
}

/// RPC Core Event Loop Handler.
#[derive(Clone)]
pub struct RpcHandler<M: Metadata = ()> {
	/// RPC Handler
	handler: Arc<MetaIoHandler<M>>,
	/// Event Loop Remote
	remote: Remote,
}

impl<M: Metadata> RpcHandler<M> {
	/// Creates new `RpcHandler` for existing `EventLoop`
	pub fn new(handler: Arc<MetaIoHandler<M>>, remote: Remote) -> Self {
		RpcHandler {
			handler: handler,
			remote: remote,
		}
	}

	/// Handles the request and returns to a closure response when it's ready.
	pub fn handle_request<F>(&self, request: &str, metadata: M, on_response: F) where
		F: Fn(Option<String>) + Send + 'static
	{
		let future = self.handler.handle_request(request, metadata);
		self.remote.spawn(|_| future.map(on_response))
	}

	/// Handles the request synchronously (not recommended)
	pub fn handle_request_sync(&self, request: &str, metadata: M) -> Option<String> {
		let (tx, rx) = mpsc::channel();
		self.handle_request(request, metadata, move |res| {
			tx.send(res).unwrap();
		});
		rx.recv().unwrap()
	}
}


