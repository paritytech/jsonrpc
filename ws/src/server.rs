use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;

use core;
use server_utils::cors::Origin;
use server_utils::hosts::{self, Host};
use server_utils::reactor::{UninitializedRemote, Remote};
use server_utils::session::SessionStats;
use ws;

use error::{Error, Result};
use metadata;
use session;

/// `WebSockets` server implementation.
pub struct Server {
	addr: SocketAddr,
	handle: Option<thread::JoinHandle<Result<()>>>,
	remote: Arc<Mutex<Option<Remote>>>,
	broadcaster: ws::Sender,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("Server")
			.field("addr", &self.addr)
			.field("handle", &self.handle)
			.field("remote", &self.remote)
			.finish()
    }
}

impl Server {
	/// Returns the address this server is listening on
	pub fn addr(&self) -> &SocketAddr {
		&self.addr
	}

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start<M: core::Metadata, S: core::Middleware<M>>(
		addr: &SocketAddr,
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<Origin>>,
		allowed_hosts: Option<Vec<Host>>,
		request_middleware: Option<Arc<session::RequestMiddleware>>,
		stats: Option<Arc<SessionStats>>,
		remote: UninitializedRemote,
		max_connections: usize,
	) -> Result<Server> {
		let config = {
			let mut config = ws::Settings::default();
			config.max_connections = max_connections;
			// don't grow non-final fragments (to prevent DOS)
			config.fragments_grow = false;
			// don't accept super large requests
			config.max_in_buffer = 5 * 1024 * 1024; // 5MB
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
		let eloop = remote.initialize()?;
		let remote = eloop.remote();

		// Create WebSocket
		let ws = ws::Builder::new().with_settings(config).build(session::Factory::new(
			handler, meta_extractor, allowed_origins, allowed_hosts, request_middleware, stats, remote
		))?;
		let broadcaster = ws.broadcaster();

		// Start listening...
		let ws = ws.bind(addr)?;
		let local_addr = ws.local_addr()?;
		debug!("Bound to local address: {}", local_addr);

		// Spawn a thread with event loop
		let handle = thread::spawn(move || {
			match ws.run().map_err(Error::from) {
				Err(error) => {
					error!("Error while running websockets server. Details: {:?}", error);
					Err(error)
				},
				Ok(_server) => Ok(()),
			}
		});

		// Return a handle
		Ok(Server {
			addr: local_addr,
			handle: Some(handle),
			remote: Arc::new(Mutex::new(Some(eloop))),
			broadcaster: broadcaster,
		})
	}
}

impl Server {
	/// Consumes the server and waits for completion
	pub fn wait(mut self) -> Result<()> {
		self.handle.take().expect("Handle is always Some at start.").join().expect("Non-panic exit")
	}

	/// Closes the server and waits for it to finish
	pub fn close(self) {
		self.close_handle().close();
	}

	/// Returns a handle to the server that can be used to close it while another thread is
	/// blocking in `wait`.
	pub fn close_handle(&self) -> CloseHandle {
		CloseHandle {
			remote: self.remote.clone(),
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
	remote: Arc<Mutex<Option<Remote>>>,
	broadcaster: ws::Sender,
}

impl CloseHandle {
	/// Closes the `Server`.
	pub fn close(self) {
		let _ = self.broadcaster.shutdown();
		self.remote.lock().unwrap().take().map(|remote| remote.close());
	}
}
