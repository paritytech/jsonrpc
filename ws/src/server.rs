use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::{cmp, fmt};

use crate::core;
use crate::server_utils::cors::Origin;
use crate::server_utils::hosts::{self, Host};
use crate::server_utils::reactor::{Executor, UninitializedExecutor};
use crate::server_utils::session::SessionStats;
use crate::ws;

use crate::error::{Error, Result};
use crate::metadata;
use crate::session;

/// `WebSockets` server implementation.
pub struct Server {
	addr: SocketAddr,
	handle: Option<thread::JoinHandle<Result<()>>>,
	executor: Arc<Mutex<Option<Executor>>>,
	broadcaster: ws::Sender,
}

impl fmt::Debug for Server {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("Server")
			.field("addr", &self.addr)
			.field("handle", &self.handle)
			.field("executor", &self.executor)
			.finish()
	}
}

impl Server {
	/// Returns the address this server is listening on
	pub fn addr(&self) -> &SocketAddr {
		&self.addr
	}

	/// Returns a Broadcaster that can be used to send messages on all connections.
	pub fn broadcaster(&self) -> Broadcaster {
		Broadcaster {
			broadcaster: self.broadcaster.clone(),
		}
	}

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start<M: core::Metadata, S: core::Middleware<M>>(
		addr: &SocketAddr,
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<dyn metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<Origin>>,
		allowed_hosts: Option<Vec<Host>>,
		request_middleware: Option<Arc<dyn session::RequestMiddleware>>,
		stats: Option<Arc<dyn SessionStats>>,
		executor: UninitializedExecutor,
		max_connections: usize,
		max_payload_bytes: usize,
		max_in_buffer_capacity: usize,
		max_out_buffer_capacity: usize,
	) -> Result<Server>
	where
		S::Future: Unpin,
		S::CallFuture: Unpin,
	{
		let config = {
			let mut config = ws::Settings::default();
			config.max_connections = max_connections;
			// don't accept super large requests
			config.max_fragment_size = max_payload_bytes;
			config.max_total_fragments_size = max_payload_bytes;
			config.in_buffer_capacity_hard_limit = max_in_buffer_capacity;
			config.out_buffer_capacity_hard_limit = max_out_buffer_capacity;
			// don't grow non-final fragments (to prevent DOS)
			config.fragments_grow = false;
			config.fragments_capacity = cmp::max(1, max_payload_bytes / config.fragment_size);
			if config.fragments_capacity > 4096 {
				config.fragments_capacity = 4096;
				config.fragments_grow = true;
			}
			// accept only handshakes beginning with GET
			config.method_strict = true;
			// require masking
			config.masking_strict = true;
			// Was shutting down server when suspending on linux:
			config.shutdown_on_interrupt = false;
			config
		};

		// Update allowed_hosts
		let allowed_hosts = hosts::update(allowed_hosts, addr);

		// Spawn event loop (if necessary)
		let eloop = executor.initialize()?;
		let executor = eloop.executor();

		// Create WebSocket
		let ws = ws::Builder::new().with_settings(config).build(session::Factory::new(
			handler,
			meta_extractor,
			allowed_origins,
			allowed_hosts,
			request_middleware,
			stats,
			executor,
		))?;
		let broadcaster = ws.broadcaster();

		// Start listening...
		let ws = ws.bind(addr)?;
		let local_addr = ws.local_addr()?;
		debug!("Bound to local address: {}", local_addr);

		// Spawn a thread with event loop
		let handle = thread::spawn(move || match ws.run().map_err(Error::from) {
			Err(error) => {
				error!("Error while running websockets server. Details: {:?}", error);
				Err(error)
			}
			Ok(_server) => Ok(()),
		});

		// Return a handle
		Ok(Server {
			addr: local_addr,
			handle: Some(handle),
			executor: Arc::new(Mutex::new(Some(eloop))),
			broadcaster,
		})
	}
}

impl Server {
	/// Consumes the server and waits for completion
	pub fn wait(mut self) -> Result<()> {
		self.handle
			.take()
			.expect("Handle is always Some at start.")
			.join()
			.expect("Non-panic exit")
	}

	/// Closes the server and waits for it to finish
	pub fn close(self) {
		self.close_handle().close();
	}

	/// Returns a handle to the server that can be used to close it while another thread is
	/// blocking in `wait`.
	pub fn close_handle(&self) -> CloseHandle {
		CloseHandle {
			executor: self.executor.clone(),
			broadcaster: self.broadcaster.clone(),
		}
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		self.close_handle().close();
		self.handle.take().map(|handle| handle.join());
	}
}

/// A handle that allows closing of a server even if it owned by a thread blocked in `wait`.
#[derive(Clone)]
pub struct CloseHandle {
	executor: Arc<Mutex<Option<Executor>>>,
	broadcaster: ws::Sender,
}

impl CloseHandle {
	/// Closes the `Server`.
	pub fn close(self) {
		let _ = self.broadcaster.shutdown();
		if let Some(executor) = self.executor.lock().unwrap().take() {
			executor.close()
		}
	}
}

/// A Broadcaster that can be used to send messages on all connections.
#[derive(Clone)]
pub struct Broadcaster {
	broadcaster: ws::Sender,
}

impl Broadcaster {
	/// Send a message to the endpoints of all connections.
	#[inline]
	pub fn send<M>(&self, msg: M) -> Result<()>
	where
		M: Into<ws::Message>,
	{
		match self.broadcaster.send(msg).map_err(Error::from) {
			Err(error) => {
				error!("Error while running sending. Details: {:?}", error);
				Err(error)
			}
			Ok(_server) => Ok(()),
		}
	}
}
