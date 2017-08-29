//! Event Loop Remote
//! Either spawns a new event loop, or re-uses provided one.

use std::{io, thread};
use std::sync::mpsc;
use tokio_core;

use core::futures::{self, Future};

/// Possibly uninitialized event loop remote.
#[derive(Debug)]
pub enum UninitializedRemote {
	/// Shared instance of remote.
	Shared(tokio_core::reactor::Remote),
	/// Event Loop should be spawned by the transport.
	Unspawned,
}

impl UninitializedRemote {
	/// Initializes remote.
	/// In case there is no shared remote, will spawn a new event loop.
	/// Dropping `Remote` closes the loop.
	pub fn initialize(self) -> io::Result<Remote> {
		self.init_with_name("event.loop")
	}

	/// Initializes remote.
	/// In case there is no shared remote, will spawn a new event loop.
	/// Dropping `Remote` closes the loop.
	pub fn init_with_name<T: Into<String>>(self, name: T) -> io::Result<Remote> {
		match self {
			UninitializedRemote::Shared(remote) => Ok(Remote::Shared(remote)),
			UninitializedRemote::Unspawned => RpcEventLoop::with_name(Some(name.into())).map(Remote::Spawned),
		}
	}
}

/// Initialized Remote
#[derive(Debug)]
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
#[derive(Debug)]
pub struct RpcEventLoop {
	remote: tokio_core::reactor::Remote,
	close: Option<futures::Complete<()>>,
	handle: Option<thread::JoinHandle<()>>,
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
		let (stop, stopped) = futures::oneshot();
		let (tx, rx) = mpsc::channel();
		let mut tb = thread::Builder::new();
		if let Some(name) = name {
			tb = tb.name(name);
		}
		let handle = tb.spawn(move || {
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
		}).expect("Couldn't spawn a thread.");
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
		self.handle.take().expect("Handle is always set before self is consumed.").join()
	}

	/// Finishes this event loop.
	pub fn close(mut self) {
		let _ = self.close.take().expect("Close is always set before self is consumed.").send(()).map_err(|e| {
			warn!("Event Loop is already finished. {:?}", e);
		});
	}
}
