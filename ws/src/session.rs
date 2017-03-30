use std;
use std::ascii::AsciiExt;
use std::sync::Arc;

use core;
use core::futures::{task, Async, Future, Poll};
use parking_lot::Mutex;
use server_utils::cors::Origin;
use server_utils::tokio_core::reactor::Remote;
use slab::Slab;
use ws;

use metadata;

/// Session id
pub type SessionId = usize;

/// Keeps track of open sessions
pub trait SessionStats: Send + Sync + 'static {
	/// Executed when new session is opened.
	fn open_session(&self, id: SessionId);
	/// Executed when session is closed.
	fn close_session(&self, id: SessionId);
}

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
		match (*self)(req) {
			Some(res) => MiddlewareAction::Respond { response: res, validate_origin: true },
			None => MiddlewareAction::Proceed,
		}
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
}

// stores two parts: liveness and parked tasks.
// the slab is only inserted into when live.
type TaskSlab = Arc<Mutex<(bool, Slab<task::Task>)>>;

// future for checking session liveness.
// this returns `NotReady` until the inner flag is `false`, and then
// returns Ready(()).
struct LivenessPoll {
	task_slab: TaskSlab,
	slab_handle: Option<usize>,
}

impl Future for LivenessPoll {
	type Item = ();
	type Error = ();

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		const INITIAL_SIZE: usize = 4;

		let mut task_slab = self.task_slab.lock();
		match task_slab.0 {
			true => {
				let task_slab = &mut task_slab.1;
				if let Some(&slab_handle) = self.slab_handle.as_ref() {
					let mut entry = task_slab.entry(slab_handle)
						.expect("slab handles are not altered by anything but the creator; qed");

					entry.replace(task::park());
				} else {
					if !task_slab.has_available() {
						// grow the size if necessary.
						// we don't expect this to get so big as to overflow.
						let reserve = ::std::cmp::max(task_slab.capacity(), INITIAL_SIZE);
						task_slab.reserve_exact(reserve);
					}

					self.slab_handle = Some(task_slab.insert(task::park())
						.expect("just grew slab; qed"));
				}

				Ok(Async::NotReady)
			}
			false => {
				if let Some(slab_handle) = self.slab_handle {
					task_slab.1.remove(slab_handle);
				}

				Ok(Async::Ready(()))
			},
		}
	}
}

impl Drop for LivenessPoll {
	fn drop(&mut self) {
		if let Some(slab_handle) = self.slab_handle {
			self.task_slab.lock().1.remove(slab_handle);
		}
	}
}

pub struct Session<M: core::Metadata, S: core::Middleware<M>> {
	context: metadata::RequestContext,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	metadata: M,
	remote: Remote,
	task_slab: TaskSlab,
}

impl<M: core::Metadata, S: core::Middleware<M>> Drop for Session<M, S> {
	fn drop(&mut self) {
		self.stats.as_ref().map(|stats| stats.close_session(self.context.session_id));

		// set the liveness flag to false.
		// and then unpark all tasks.
		let mut task_slab = self.task_slab.lock();
		task_slab.0 = false;

		for task in task_slab.1.iter() {
			task.unpark()
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> Session<M, S> {
	fn verify_origin(&self, req: &ws::Request) -> Option<ws::Response> {
		let origin = req.header("origin").map(|x| &x[..]);
		if !origin_is_allowed(&self.allowed_origins, origin) {
			warn!(target: "signer", "Blocked connection to Signer API from untrusted origin: {:?}", origin);
			Some(forbidden(
				"URL Blocked",
				"Connection Origin has been rejected.",
			))
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

		if action.should_verify_origin() {
			// Verify request origin.
			if let Some(response) = self.verify_origin(req) {
				return Ok(response);
			}
		}

		self.metadata = self.meta_extractor.extract_metadata(&self.context);

		match action {
			MiddlewareAction::Proceed => ws::Response::from_request(req),
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
						warn!(target: "signer", "Error while sending response: {:?}", e);
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
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	remote: Remote,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<Origin>>,
		request_middleware: Option<Arc<RequestMiddleware>>,
		stats: Option<Arc<SessionStats>>,
		remote: Remote,
	) -> Self {
		Factory {
			session_id: 0,
			handler: handler,
			meta_extractor: meta_extractor,
			allowed_origins: allowed_origins,
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

		Session {
			context: metadata::RequestContext {
				session_id: self.session_id,
				out: sender,
			},
			handler: self.handler.clone(),
			meta_extractor: self.meta_extractor.clone(),
			allowed_origins: self.allowed_origins.clone(),
			stats: self.stats.clone(),
			request_middleware: self.request_middleware.clone(),
			metadata: Default::default(),
			remote: self.remote.clone(),
			task_slab: Arc::new(Mutex::new((true, Slab::with_capacity(0)))),
		}
	}
}

fn origin_is_allowed(allowed_origins: &Option<Vec<Origin>>, header: Option<&[u8]>) -> bool {
	let header = header.map(std::str::from_utf8);

	match (header, allowed_origins.as_ref()) {
		// Always allow if Origin is not specified
		(None, _) => true,
		// Always allow if Origin validation is disabled
		(_, None) => true,
		// Validate Origin
		(Some(Ok(origin)), Some(origins)) => {
			for o in origins {
				if origin.eq_ignore_ascii_case(&o) {
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
