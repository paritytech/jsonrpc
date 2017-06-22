use std;
use std::sync::{atomic, Arc};

use core;
use core::futures::{task, Async, Future, Poll};
use core::futures::sync::BiLock;

use parking_lot::Mutex;
use slab::Slab;

use server_utils::Pattern;
use server_utils::cors::Origin;
use server_utils::hosts::Host;
use server_utils::tokio_core::reactor::Remote;
use server_utils::session::{SessionId, SessionStats};
use ws;

use metadata;

/// Middleware to intercept server requests.
/// You can either terminate the request (by returning a response)
/// or just proceed with standard JSON-RPC handling.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Process a request and decide what to do next.
	fn process(&self, req: &ws::Request) -> MiddlewareAction;
}

impl<F> RequestMiddleware for F where
	F: Fn(&ws::Request) -> Option<ws::Response> + Send + Sync + 'static,
{
	fn process(&self, req: &ws::Request) -> MiddlewareAction {
		(*self)(req).into()
	}
}

/// Request middleware action
pub enum MiddlewareAction {
	/// Proceed with standard JSON-RPC behaviour.
	Proceed,
	/// Terminate the request and return a response.
	Respond {
		/// Response to return
		response: ws::Response,
		/// Should origin be validated before returning the response?
		validate_origin: bool,
		/// Should hosts be validated before returning the response?
		validate_hosts: bool,
	},
}

impl MiddlewareAction {
	fn should_verify_origin(&self) -> bool {
		use self::MiddlewareAction::*;

		match *self {
			Proceed => true,
			Respond { validate_origin, .. } => validate_origin,
		}
	}

	fn should_verify_hosts(&self) -> bool {
		use self::MiddlewareAction::*;

		match *self {
			Proceed => true,
			Respond { validate_hosts, .. } => validate_hosts,
		}
	}
}

impl From<Option<ws::Response>> for MiddlewareAction {
	fn from(opt: Option<ws::Response>) -> Self {
		match opt {
			Some(res) => MiddlewareAction::Respond { response: res, validate_origin: true, validate_hosts: true },
			None => MiddlewareAction::Proceed,
		}
	}
}

// the slab is only inserted into when live.
type TaskSlab = Option<Slab<task::Task>>;

// owns both sides of a `BiLock`ed task slab: one side is used within
// futures to register their task. the other side is used in synchronous
// settings like dropping a `LivenessPoll` or the `Session`.
struct SharedTaskSlab {
	async: BiLock<TaskSlab>,
	blocking: Mutex<Option<BiLock<TaskSlab>>>,
}

impl SharedTaskSlab {
	fn new() -> Self {
		let (side_a, side_b) = BiLock::new(Some(Slab::with_capacity(0)));

		SharedTaskSlab {
			async: side_a,
			blocking: Mutex::new(Some(side_b))
		}
	}

	// access the slab in a blocking manner.
	fn with_blocking<T, F>(&self, f: F) -> T where F: FnOnce(&mut TaskSlab) -> T {
		// get handle to half of the BiLock and acquire it synchronously.
		let mut lock = self.blocking.lock();
		let mut slab_handle = lock.take().expect("ownership always restored after locking; qed")
			.lock()
			.wait()
			.expect("BiLockAcquire always resolves `Ok`; qed");

		// call the closure and release the lock.
		let res = f(&mut *slab_handle);
		*lock = Some(slab_handle.unlock());

		res
	}

	// get access to the async side of the BiLock.
	fn async(&self) -> &BiLock<TaskSlab> {
		&self.async
	}
}

// future for checking session liveness.
// this returns `NotReady` until the inner flag is `false`, and then
// returns Ready(()).
struct LivenessPoll {
	task_slab: Arc<SharedTaskSlab>,
	slab_handle: Option<usize>,
}

impl Future for LivenessPoll {
	type Item = ();
	type Error = ();

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		const INITIAL_SIZE: usize = 4;

		let mut task_slab = match self.task_slab.async().poll_lock() {
			Async::Ready(locked) => locked,
			Async::NotReady => return Ok(Async::NotReady),
		};

		match (task_slab.as_mut(), self.slab_handle.clone()) {
			(None, _) => Ok(Async::Ready(())), // not live means we need to resolve this future.
			(Some(task_slab), Some(slab_handle)) => {
				let mut entry = task_slab.entry(slab_handle)
					.expect("slab handles are not altered by anything but the creator; qed");

				entry.replace(task::current());
				Ok(Async::NotReady)
			}
			(Some(task_slab), None) => {
				if !task_slab.has_available() {
					// grow the size if necessary.
					// we don't expect this to get so big as to overflow.
					let reserve = ::std::cmp::max(task_slab.capacity(), INITIAL_SIZE);
					task_slab.reserve_exact(reserve);
				}

				self.slab_handle = Some(task_slab.insert(task::current())
					.expect("just grew slab; qed"));
				Ok(Async::NotReady)
			}
		}
	}
}

impl Drop for LivenessPoll {
	fn drop(&mut self) {
		// remove the entry from the slab if it hasn't been destroyed yet.
		if let Some(slab_handle) = self.slab_handle {
			self.task_slab.with_blocking(|task_slab| {
				if let Some(slab) = task_slab.as_mut() {
					slab.remove(slab_handle);
				}
			})
		}
	}
}

pub struct Session<M: core::Metadata, S: core::Middleware<M>> {
	active: Arc<atomic::AtomicBool>,
	context: metadata::RequestContext,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	allowed_hosts: Option<Vec<Host>>,
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	metadata: M,
	remote: Remote,
	task_slab: Arc<SharedTaskSlab>,
}

impl<M: core::Metadata, S: core::Middleware<M>> Drop for Session<M, S> {
	fn drop(&mut self) {
		self.active.store(false, atomic::Ordering::SeqCst);
		self.stats.as_ref().map(|stats| stats.close_session(self.context.session_id));

		// destroy the shared handle and unpark all tasks.
		let tasks_to_unpark = self.task_slab.with_blocking(|task_slab| {
			task_slab.take()
				.expect("only set to `None` in this destructor. destructors are never called twice; qed")
		});

		for task in tasks_to_unpark.iter() {
			task.notify();
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> Session<M, S> {
	fn read_origin<'a>(&self, req: &'a ws::Request) -> Option<&'a [u8]> {
		req.header("origin").map(|x| &x[..])
	}

	fn verify_origin(&self, origin: Option<&[u8]>) -> Option<ws::Response> {
		if !header_is_allowed(&self.allowed_origins, origin) {
			warn!(
				"Blocked connection to WebSockets server from untrusted origin: {:?}",
				origin.and_then(|s| std::str::from_utf8(s).ok()),
			);
			Some(forbidden("URL Blocked", "Connection Origin has been rejected."))
		} else {
			None
		}
	}

	fn verify_host(&self, req: &ws::Request) -> Option<ws::Response> {
		let host = req.header("host").map(|x| &x[..]);
		if !header_is_allowed(&self.allowed_hosts, host) {
			warn!(
				"Blocked connection to WebSockets server with untrusted host: {:?}",
				host.and_then(|s| std::str::from_utf8(s).ok()),
			);
			Some(forbidden("URL Blocked", "Connection Host has been rejected."))
		} else {
			None
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ws::Handler for Session<M, S> {
	fn on_request(&mut self, req: &ws::Request) -> ws::Result<ws::Response> {
		// Run middleware
		let action = if let Some(ref middleware) = self.request_middleware {
			middleware.process(req)
		} else {
			MiddlewareAction::Proceed
		};

		let origin = self.read_origin(req);
		if action.should_verify_origin() {
			// Verify request origin.
			if let Some(response) = self.verify_origin(origin) {
				return Ok(response);
			}
		}

		if action.should_verify_hosts() {
			// Verify host header.
			if let Some(response) = self.verify_host(req) {
				return Ok(response);
			}
		}

		self.context.origin = origin.and_then(|origin| ::std::str::from_utf8(origin).ok()).map(Into::into);
		self.context.protocols = req.protocols().ok()
			.map(|protos| protos.into_iter().map(Into::into).collect())
			.unwrap_or_else(Vec::new);
		self.metadata = self.meta_extractor.extract(&self.context);

		match action {
			MiddlewareAction::Proceed => ws::Response::from_request(req).map(|mut res| {
				if let Some(protocol) = self.context.protocols.get(0) {
					res.set_protocol(protocol);
				}
				res
			}),
			MiddlewareAction::Respond { response, .. } => Ok(response),
		}
	}

	fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
		let req = msg.as_text()?;
		let out = self.context.out.clone();
		let metadata = self.metadata.clone();

		let poll_liveness = LivenessPoll {
			task_slab: self.task_slab.clone(),
			slab_handle: None,
		};

		let future = self.handler.handle_request(req, metadata)
			.map(move |response| {
				if let Some(result) = response {
					let res = out.send(result);
					if let Err(e) = res {
						warn!("Error while sending response: {:?}", e);
					}
				}
			})
			.select(poll_liveness)
			.map(|_| ())
			.map_err(|_| ());

		self.remote.spawn(|_| future);

		Ok(())
	}
}

pub struct Factory<M: core::Metadata, S: core::Middleware<M>> {
	session_id: SessionId,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	allowed_hosts: Option<Vec<Host>>,
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	remote: Remote,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<Origin>>,
		allowed_hosts: Option<Vec<Host>>,
		request_middleware: Option<Arc<RequestMiddleware>>,
		stats: Option<Arc<SessionStats>>,
		remote: Remote,
	) -> Self {
		Factory {
			session_id: 0,
			handler: handler,
			meta_extractor: meta_extractor,
			allowed_origins: allowed_origins,
			allowed_hosts: allowed_hosts,
			request_middleware: request_middleware,
			stats: stats,
			remote: remote,
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ws::Factory for Factory<M, S> {
	type Handler = Session<M, S>;

	fn connection_made(&mut self, sender: ws::Sender) -> Self::Handler {
		self.session_id += 1;
		self.stats.as_ref().map(|stats| stats.open_session(self.session_id));
		let active = Arc::new(atomic::AtomicBool::new(true));

		Session {
			active: active.clone(),
			context: metadata::RequestContext {
				session_id: self.session_id,
				origin: None,
				protocols: Vec::new(),
				out: metadata::Sender::new(sender, active),
				remote: self.remote.clone(),
			},
			handler: self.handler.clone(),
			meta_extractor: self.meta_extractor.clone(),
			allowed_origins: self.allowed_origins.clone(),
			allowed_hosts: self.allowed_hosts.clone(),
			stats: self.stats.clone(),
			request_middleware: self.request_middleware.clone(),
			metadata: Default::default(),
			remote: self.remote.clone(),
			task_slab: Arc::new(SharedTaskSlab::new()),
		}
	}
}

fn header_is_allowed<T>(allowed: &Option<Vec<T>>, header: Option<&[u8]>) -> bool where
	T: Pattern,
{
	let header = header.map(std::str::from_utf8);

	match (header, allowed.as_ref()) {
		// Always allow if Origin/Host is not specified
		(None, _) => true,
		// Always allow if Origin/Host validation is disabled
		(_, None) => true,
		// Validate Origin
		(Some(Ok(val)), Some(values)) => {
			for v in values {
				if v.matches(val) {
					return true
				}
			}
			false
		},
		// Disallow in other cases
		_ => false,
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
