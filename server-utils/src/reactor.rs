//! Event Loop Remote
//! Either spawns a new event loop, or re-uses provided one.

use std::{io, thread};
use std::sync::mpsc;
use tokio_core;

use core::futures::{self, Future};

/// Possibly uninitialized event loop remote.
pub enum UnitializedRemote {
	Shared(tokio_core::reactor::Remote),
	Unspawned,
}

impl UnitializedRemote {
	/// Initializes remote.
	/// In case there is no shared remote, will spawn a new event loop.
	/// Dropping `Remote` closes the loop.
	pub fn initialize(self) -> io::Result<Remote> {
		match self {
			UnitializedRemote::Shared(remote) => Ok(Remote::Shared(remote)),
			UnitializedRemote::Unspawned => RpcEventLoop::spawn().map(Remote::Spawned),
		}
	}
}

/// Initialized Remote
pub enum Remote {
	/// Shared instance
	Shared(tokio_core::reactor::Remote),
	/// Spawned Event Loop
	Spawned(RpcEventLoop),
}

impl Remote {
	/// Get remote associated with this event loop.
	pub fn remote(&self) -> tokio_core::reactor::Remote {
		match *self {
			Remote::Shared(ref remote) => remote.clone(),
			Remote::Spawned(ref eloop) => eloop.remote(),
		}
	}

	/// Closes underlying event loop (if any!).
	pub fn close(self) {
		if let Remote::Spawned(eloop) = self {
			eloop.close()
		}
	}

	/// Wait for underlying event loop to finish (if any!).
	pub fn wait(self) {
		if let Remote::Spawned(eloop) = self {
			let _ = eloop.wait();
		}
	}
}

/// A handle to running event loop. Dropping the handle will cause event loop to finish.
pub struct RpcEventLoop {
	remote: tokio_core::reactor::Remote,
	close: Option<futures::Complete<()>>,
	handle: Option<thread::JoinHandle<()>>,
}

impl Drop for RpcEventLoop {
	fn drop(&mut self) {
		self.close.take().map(|v| v.complete(()));
	}
}

impl RpcEventLoop {
	/// Spawns a new thread with `EventLoop` with given handler.
	pub fn spawn() -> io::Result<Self> {
		let (stop, stopped) = futures::oneshot();
		let (tx, rx) = mpsc::channel();
		let handle = thread::spawn(move || {
			let el = tokio_core::reactor::Core::new();
			match el {
				Ok(mut el) => {
					tx.send(Ok(el.remote())).expect("Rx is blocking upper thread.");
					let _ = el.run(futures::empty().select(stopped));
				},
				Err(err) => {
					tx.send(Err(err)).expect("Rx is blocking upper thread.");
				}
			}
		});
		let remote = rx.recv().expect("tx is transfered to a newly spawned thread.");

		remote.map(|remote| RpcEventLoop {
			remote: remote,
			close: Some(stop),
			handle: Some(handle),
		})
	}

	/// Get remote for this event loop.
	pub fn remote(&self) -> tokio_core::reactor::Remote {
		self.remote.clone()
	}

	/// Blocks current thread and waits until the event loop is finished.
	pub fn wait(mut self) -> thread::Result<()> {
		self.handle.take().unwrap().join()
	}

	/// Finishes this event loop.
	pub fn close(mut self) {
		self.close.take().unwrap().complete(())
	}
}
