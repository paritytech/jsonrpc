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
pub struct EventLoop<M: Metadata = ()> {
	rpc: RpcHandler<M>,
	handle: EventLoopHandle,
}

impl<M: Metadata> EventLoop<M> {
	/// Spawns a new thread with `EventLoop` with given handler.
	pub fn spawn(handler: Arc<MetaIoHandler<M>>) -> EventLoop<M> {
		let (stop, stopped) = futures::oneshot();
		let (tx, rx) = mpsc::channel();
		let handle = thread::spawn(move || {
			let mut el = tokio_core::reactor::Core::new().expect("Creating an event loop should not fail.");
			tx.send(el.remote()).expect("Rx is blocking upper thread.");
			let _ = el.run(futures::empty().select(stopped));
		});
		let remote = rx.recv().expect("tx is transfered to a newly spawned thread.");

		EventLoop {
			rpc: RpcHandler::new(handler, remote),
			handle: EventLoopHandle {
				close: Some(stop),
				handle: Some(handle),
			},
		}
	}

	/// Returns an RPC handler to process requests.
	pub fn handler(&self) -> RpcHandler<M> {
		self.rpc.clone()
	}
}

/// A handle to running event loop. Dropping the handle will cause event loop to finish.
pub struct EventLoopHandle {
	close: Option<futures::Complete<()>>,
	handle: Option<thread::JoinHandle<()>>
}

impl<M: Metadata> From<EventLoop<M>> for EventLoopHandle {
	fn from(el: EventLoop<M>) -> Self {
		el.handle
	}
}

impl Drop for EventLoopHandle {
	fn drop(&mut self) {
		self.close.take().map(|v| v.complete(()));
	}
}

impl EventLoopHandle {
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
}


