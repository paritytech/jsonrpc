//! Event Loop Executor
//!
//! Either spawns a new event loop, or re-uses provided one.
//! Spawned event loop is always single threaded (mostly for
//! historical/backward compatibility reasons) despite the fact
//! that `tokio::runtime` can be multi-threaded.

use std::io;

use tokio::runtime;
/// Task executor for Tokio 0.2 runtime.
pub type TaskExecutor = tokio::runtime::Handle;

/// Possibly uninitialized event loop executor.
#[derive(Debug)]
pub enum UninitializedExecutor {
	/// Shared instance of executor.
	Shared(TaskExecutor),
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
	Shared(TaskExecutor),
	/// Spawned Event Loop
	Spawned(RpcEventLoop),
}

impl Executor {
	/// Get tokio executor associated with this event loop.
	pub fn executor(&self) -> TaskExecutor {
		match self {
			Executor::Shared(ref executor) => executor.clone(),
			Executor::Spawned(ref eloop) => eloop.executor(),
		}
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
}

/// A handle to running event loop. Dropping the handle will cause event loop to finish.
#[derive(Debug)]
pub struct RpcEventLoop {
	executor: TaskExecutor,
	close: Option<futures::channel::oneshot::Sender<()>>,
	runtime: Option<runtime::Runtime>,
}

impl Drop for RpcEventLoop {
	fn drop(&mut self) {
		self.close.take().map(|v| v.send(()));
	}
}

impl RpcEventLoop {
	/// Spawns a new thread with the `EventLoop`.
	pub fn spawn() -> io::Result<Self> {
		RpcEventLoop::with_name(None)
	}

	/// Spawns a new named thread with the `EventLoop`.
	pub fn with_name(name: Option<String>) -> io::Result<Self> {
		let (stop, stopped) = futures::channel::oneshot::channel();

		let mut tb = runtime::Builder::new();
		tb.core_threads(1);
		tb.threaded_scheduler();
		tb.enable_all();

		if let Some(name) = name {
			tb.thread_name(name);
		}

		let runtime = tb.build()?;
		let executor = runtime.handle().to_owned();

		runtime.spawn(async {
			let _ = stopped.await;
		});

		Ok(RpcEventLoop {
			executor,
			close: Some(stop),
			runtime: Some(runtime),
		})
	}

	/// Get executor for this event loop.
	pub fn executor(&self) -> runtime::Handle {
		self.runtime
			.as_ref()
			.expect("Runtime is only None if we're being dropped; qed")
			.handle()
			.clone()
	}

	/// Blocks current thread and waits until the event loop is finished.
	pub fn wait(mut self) -> Result<(), ()> {
		// Dropping Tokio 0.2 runtime waits for all spawned tasks to terminate
		let runtime = self.runtime.take().ok_or(())?;
		drop(runtime);
		Ok(())
	}

	/// Finishes this event loop.
	pub fn close(mut self) {
		let _ = self
			.close
			.take()
			.expect("Close is always set before self is consumed.")
			.send(())
			.map_err(|e| {
				warn!("Event Loop is already finished. {:?}", e);
			});
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
