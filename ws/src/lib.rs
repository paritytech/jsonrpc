//! `WebSockets` server.

extern crate jsonrpc_core as core;
extern crate ws;

#[macro_use]
extern crate log;

use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;

use core::futures::Future;

/// Signer startup error
#[derive(Debug)]
pub enum ServerError {
	/// Wrapped `std::io::Error`
	IoError(std::io::Error),
	/// Other `ws-rs` error
	WebSocket(ws::Error)
}

impl From<ws::Error> for ServerError {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(e) => ServerError::IoError(e),
			_ => ServerError::WebSocket(err),
		}
	}
}

/// Request context
pub struct RequestContext {
	/// Direct channel to send messages to a client.
	pub out: ws::Sender,
}

/// Metadata extractor from session data.
pub trait MetaExtractor<M: core::Metadata>: Send + Sync + 'static {
	/// Extract metadata for given session
	fn extract_metadata(&self, _context: &RequestContext) -> M {
		Default::default()
	}
}

/// Dummy metadata extractor
#[derive(Clone)]
pub struct NoopExtractor;
impl<M: core::Metadata> MetaExtractor<M> for NoopExtractor {}

/// Builder for `WebSockets` server
pub struct ServerBuilder<M: core::Metadata, S: core::Middleware<M>> {
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
}

impl<M: core::Metadata, S: core::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder`
	pub fn new<T>(handler: T) -> Self where
		T: Into<core::MetaIoHandler<M, S>>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			allowed_origins: None,
		}
	}

	/// Sets a meta extractor.
	pub fn session_meta_extractor<T: MetaExtractor<M>>(mut self, extractor: T) -> Self {
		self.meta_extractor = Arc::new(extractor);
		self
	}

	/// Allowed origins.
	pub fn allowed_origins(mut self, allowed_origins: Option<Vec<String>>) -> Self {
		self.allowed_origins = allowed_origins;
		self
	}

	// TODO [ToDr] Session statistics
	// TODO [ToDr] Connection middleware

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start(self, addr: &SocketAddr) -> Result<Server, ServerError> {
		Server::start(
			addr,
			self.handler,
			self.meta_extractor,
			self.allowed_origins,
		)
	}

}

/// `WebSockets` server implementation.
pub struct Server {
	addr: SocketAddr,
	handle: Option<thread::JoinHandle<Result<(), ServerError>>>,
	broadcaster: ws::Sender,
}

impl Server {
	/// Returns the address this server is listening on
	pub fn addr(&self) -> &SocketAddr {
		&self.addr
	}

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	fn start<M: core::Metadata, S: core::Middleware<M>>(
		addr: &SocketAddr,
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<MetaExtractor<M>>,
		allowed_origins: Option<Vec<String>>,
	) -> Result<Server, ServerError> {
		let config = {
			let mut config = ws::Settings::default();
			// accept only handshakes beginning with GET
			config.method_strict = true;
			// Was shutting down server when suspending on linux:
			config.shutdown_on_interrupt = false;
			config
		};

		// Create WebSocket
		let ws = ws::Builder::new().with_settings(config).build(
			Factory::new(handler, meta_extractor, allowed_origins)
		)?;
		let broadcaster = ws.broadcaster();

		// Start listening...
		let ws = ws.bind(addr)?;
		// Spawn a thread with event loop
		let handle = thread::spawn(move || {
			match ws.run().map_err(ServerError::from) {
				Err(error) => {
					error!("Error while running websockets server. Details: {:?}", error);
					Err(error)
				},
				Ok(_server) => Ok(()),
			}
		});

		// Return a handle
		Ok(Server {
			addr: addr.to_owned(),
			handle: Some(handle),
			broadcaster: broadcaster,
		})
	}
}

impl Server {
	/// Consumes the server and waits for completion
	pub fn wait(mut self) -> Result<(), ServerError> {
		self.handle.take().unwrap().join().unwrap()
	}

	/// Closes the server and waits for it to finish
	pub fn close(self) {
		let _ = self.broadcaster.shutdown();
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.broadcaster.shutdown();
		self.handle.take().map(|handle| handle.join());
	}
}

fn origin_is_allowed(allowed_origins: &Option<Vec<String>>, header: Option<&[u8]>) -> bool {
	if let Some(origins) = allowed_origins.as_ref() {
		if let Some(Ok(origin)) = header.map(|h| std::str::from_utf8(h)) {
			for o in origins {
				if o == origin {
					return true
				}
			}
		}
		false
	} else {
		// Allow all origins if validation is disabled.
		true
	}
}


fn forbidden(title: &str, message: &str) -> ws::Response {
	let mut forbidden = ws::Response::new(403, "Forbidden");
	forbidden.set_body(
		format!("{}\n{}\n", title, message).as_bytes()
	);
	{
		let mut headers = forbidden.headers_mut();
		headers.push(("Connection".to_owned(), "close".as_bytes().to_vec()));
	}
	forbidden
}

pub struct Session<M: core::Metadata, S: core::Middleware<M>> {
	out: ws::Sender,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
	metadata: M,
}

impl<M: core::Metadata, S: core::Middleware<M>> Drop for Session<M, S> {
	fn drop(&mut self) {
		// self.stats.as_ref().map(|stats| stats.close_session());
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ws::Handler for Session<M, S> {
	fn on_request(&mut self, req: &ws::Request) -> ws::Result<ws::Response> {
		// Check request origin and host header.
		let origin = req.header("origin").or_else(|| req.header("Origin")).map(|x| &x[..]);

		if !origin_is_allowed(&self.allowed_origins, origin) {
			warn!(target: "signer", "Blocked connection to Signer API from untrusted origin: {:?}", origin);
			return Ok(forbidden(
				"URL Blocked",
				"Connection Origin has been rejected.",
			));
		}
		let context = RequestContext {
			out: self.out.clone(),
		};
		self.metadata = self.meta_extractor.extract_metadata(&context);
		return ws::Response::from_request(req)
	}

	fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
		let req = msg.as_text()?;
		let out = self.out.clone();
		let metadata = self.metadata.clone();

		self.handler.handle_request(req, metadata)
			.wait()
			.map_err(|_| unreachable!())
			.map(move |response| {
				if let Some(result) = response {
					let res = out.send(result);
					if let Err(e) = res {
						warn!(target: "signer", "Error while sending response: {:?}", e);
					}
				}
			})
	}
}

pub struct Factory<M: core::Metadata, S: core::Middleware<M>> {
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<MetaExtractor<M>>,
		allowed_origins: Option<Vec<String>>,
	) -> Self {
		Factory {
			handler: handler,
			meta_extractor: meta_extractor,
			allowed_origins: allowed_origins,
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ws::Factory for Factory<M, S> {
	type Handler = Session<M, S>;

	fn connection_made(&mut self, sender: ws::Sender) -> Self::Handler {
		// self.stats.as_ref().map(|stats| stats.open_session());

		Session {
			out: sender,
			handler: self.handler.clone(),
			meta_extractor: self.meta_extractor.clone(),
			allowed_origins: self.allowed_origins.clone(),
			metadata: Default::default(),
		}
	}
}
