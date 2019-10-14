//! Event Loop Executor
//!
//! Either spawns a new event loop, or re-uses provided one.
//! Spawned event loop is always single threaded (mostly for
//! historical/backward compatibility reasons) despite the fact
//! that `tokio::runtime` can be multi-threaded.

use std::io;
use std::sync::Arc;

use tokio;
use parking_lot::Mutex;

use crate::core::futures::{self, Future};

/// Possibly uninitialized event loop executor.
#[derive(Debug)]
pub enum UninitializedExecutor {
	/// Shared instance of executor.
	Shared(tokio::runtime::TaskExecutor),
	/// Event Loop should be spawned by the transport.
	Unspawned,
}

impl UninitializedExecutor {
	/// Initializes executor.
	/// In case there is no shared executor, will spawn a new event loop.
	/// Dropping `Executor` closes the loop.
	pub fn initialize(self) -> io::Result<Executor> {
		self.init_with_name("event.loop")
	}

	/// Initializes executor.
	/// In case there is no shared executor, will spawn a new event loop.
	/// Dropping `Executor` closes the loop.
	pub fn init_with_name<T: Into<String>>(self, name: T) -> io::Result<Executor> {
		match self {
			UninitializedExecutor::Shared(executor) => Ok(Executor::Shared(executor)),
			UninitializedExecutor::Unspawned => RpcEventLoop::with_name(Some(name.into())).map(Executor::Spawned),
		}
	}
}

/// Initialized Executor
#[derive(Debug)]
pub enum Executor {
	/// Shared instance
	Shared(tokio::runtime::TaskExecutor),
	/// Spawned Event Loop
	Spawned(RpcEventLoop),
}

impl Executor {
	/// Get tokio executor associated with this event loop.
	pub fn executor(&self) -> tokio::runtime::TaskExecutor {
		match *self {
			Executor::Shared(ref executor) => executor.clone(),
			Executor::Spawned(ref eloop) => eloop.executor(),
		}
	}

	/// Spawn a future onto the Tokio runtime.
	pub fn spawn<F>(&self, future: F)
	where
		F: Future<Item = (), Error = ()> + Send + 'static,
	{
		self.executor().spawn(future)
	}

	/// Closes underlying event loop (if any!).
	pub fn close(self) {
		if let Executor::Spawned(eloop) = self {
			eloop.close()
		}
	}

	/// Wait for underlying event loop to finish (if any!).
	pub fn wait(self) {
		if let Executor::Spawned(eloop) = self {
			let _ = eloop.wait();
		}
	}

	/// Creates a close handle that can be used to stop the server remotely
	pub fn close_handle(&self) -> ExecutorCloseHandle {
		let inner_handle = if let Executor::Spawned(eloop) = self {
			Some(eloop.close_handle())
		} else {
			None
		};
		ExecutorCloseHandle { inner_handle }
	}
}

/// `CloseHandle` allows one to stop an `Executor` remotely.
#[derive(Debug, Clone)]
pub struct ExecutorCloseHandle {
	inner_handle: Option<RpcEventLoopCloseHandle>,
}

impl ExecutorCloseHandle {
	/// `close` closes the corresponding `Executor` instance.
	pub fn close(mut self) {
		if let Some(closer) = self.inner_handle.take() {
			closer.close()
		}
	}
}

/// A handle to running event loop. Dropping the handle will cause event loop to finish.
#[derive(Debug)]
pub struct RpcEventLoop {
	executor: tokio::runtime::TaskExecutor,
	close_handle: RpcEventLoopCloseHandle,
	handle: Option<tokio::runtime::Shutdown>,
}

impl Drop for RpcEventLoop {
	fn drop(&mut self) {
		self.close_handle().close()
	}
}

impl RpcEventLoop {
	/// Spawns a new thread with the `EventLoop`.
	pub fn spawn() -> io::Result<Self> {
		RpcEventLoop::with_name(None)
	}

	/// Spawns a new named thread with the `EventLoop`.
	pub fn with_name(name: Option<String>) -> io::Result<Self> {
		let (stop, stopped) = futures::oneshot();

		let mut tb = tokio::runtime::Builder::new();
		tb.core_threads(1);

		if let Some(name) = name {
			tb.name_prefix(name);
		}

		let mut runtime = tb.build()?;
		let executor = runtime.executor();
		let terminate = futures::empty().select(stopped).map(|_| ()).map_err(|_| ());
		runtime.spawn(terminate);
		let handle = runtime.shutdown_on_idle();

		Ok(RpcEventLoop {
			executor,
			close_handle: RpcEventLoopCloseHandle {
				closer: Arc::new(Mutex::new(Some(stop))),
			},
			handle: Some(handle),
		})
	}

	/// Get executor for this event loop.
	pub fn executor(&self) -> tokio::runtime::TaskExecutor {
		self.executor.clone()
	}

	/// Blocks current thread and waits until the event loop is finished.
	pub fn wait(mut self) -> Result<(), ()> {
		self.handle
			.take()
			.ok_or(())?
			.wait()
	}

	/// Finishes this event loop.
	pub fn close(self) {
		self.close_handle().close()
	}

	/// close handle
	pub fn close_handle(&self) -> RpcEventLoopCloseHandle {
		self.close_handle.clone()
	}
}

/// `CloseHandle` allows one to stop an `RpcEventLoop` remotely.
#[derive(Debug, Clone)]
pub struct RpcEventLoopCloseHandle {
	closer: Arc<Mutex<Option<futures::Complete<()>>>>,
}

impl RpcEventLoopCloseHandle {
	/// `close` closes the corresponding `RpcEventLoop` instance.
	pub fn close(self) {
		if let Some(closer) = self.closer.lock().take() {
			let _ = closer.send(()).map_err(|e| {
				warn!("Event Loop is already finished. {:?}", e);
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn make_sure_rpc_event_loop_is_send_and_sync() {
		fn is_send_and_sync<T: Send + Sync>() {}

		is_send_and_sync::<RpcEventLoop>();
	}
}
