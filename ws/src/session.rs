use std::future::Future;
use std::pin::Pin;
use std::sync::{atomic, Arc};
use std::task::{Context, Poll};

use crate::core;
use futures::channel::oneshot;
use futures::future;
use futures::FutureExt;

use parking_lot::Mutex;
use slab::Slab;

use crate::server_utils::cors::Origin;
use crate::server_utils::hosts::Host;
use crate::server_utils::reactor::TaskExecutor;
use crate::server_utils::session::{SessionId, SessionStats};
use crate::server_utils::Pattern;
use crate::ws;

use crate::error;
use crate::metadata;

/// Middleware to intercept server requests.
/// You can either terminate the request (by returning a response)
/// or just proceed with standard JSON-RPC handling.
pub trait RequestMiddleware: Send + Sync + 'static {
	/// Process a request and decide what to do next.
	fn process(&self, req: &ws::Request) -> MiddlewareAction;
}

impl<F> RequestMiddleware for F
where
	F: Fn(&ws::Request) -> Option<ws::Response> + Send + Sync + 'static,
{
	fn process(&self, req: &ws::Request) -> MiddlewareAction {
		(*self)(req).into()
	}
}

/// Request middleware action
#[derive(Debug)]
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
			Some(res) => MiddlewareAction::Respond {
				response: res,
				validate_origin: true,
				validate_hosts: true,
			},
			None => MiddlewareAction::Proceed,
		}
	}
}

// the slab is only inserted into when live.
type TaskSlab = Mutex<Slab<Option<oneshot::Sender<()>>>>;

// future for checking session liveness.
// this returns `NotReady` until the session it corresponds to is dropped.
#[derive(Debug)]
struct LivenessPoll {
	task_slab: Arc<TaskSlab>,
	slab_handle: usize,
	rx: oneshot::Receiver<()>,
}

impl LivenessPoll {
	fn create(task_slab: Arc<TaskSlab>) -> Self {
		const INITIAL_SIZE: usize = 4;

		let (index, rx) = {
			let mut task_slab = task_slab.lock();
			if task_slab.len() == task_slab.capacity() {
				// grow the size if necessary.
				// we don't expect this to get so big as to overflow.
				let reserve = ::std::cmp::max(task_slab.capacity(), INITIAL_SIZE);
				task_slab.reserve_exact(reserve);
			}

			let (tx, rx) = oneshot::channel();
			let index = task_slab.insert(Some(tx));
			(index, rx)
		};

		LivenessPoll {
			task_slab,
			slab_handle: index,
			rx,
		}
	}
}

impl Future for LivenessPoll {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = Pin::into_inner(self);
		// if the future resolves ok then we've been signalled to return.
		// it should never be cancelled, but if it was the session definitely
		// isn't live.
		match Pin::new(&mut this.rx).poll(cx) {
			Poll::Ready(_) => Poll::Ready(()),
			Poll::Pending => Poll::Pending,
		}
	}
}

impl Drop for LivenessPoll {
	fn drop(&mut self) {
		// remove the entry from the slab if it hasn't been destroyed yet.
		self.task_slab.lock().remove(self.slab_handle);
	}
}

pub struct Session<M: core::Metadata, S: core::Middleware<M>> {
	active: Arc<atomic::AtomicBool>,
	context: metadata::RequestContext,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	allowed_hosts: Option<Vec<Host>>,
	request_middleware: Option<Arc<dyn RequestMiddleware>>,
	stats: Option<Arc<dyn SessionStats>>,
	metadata: Option<M>,
	executor: TaskExecutor,
	task_slab: Arc<TaskSlab>,
}

impl<M: core::Metadata, S: core::Middleware<M>> Drop for Session<M, S> {
	fn drop(&mut self) {
		self.active.store(false, atomic::Ordering::Relaxed);
		if let Some(stats) = self.stats.as_ref() {
			stats.close_session(self.context.session_id)
		}

		// signal to all still-live tasks that the session has been dropped.
		for (_index, task) in self.task_slab.lock().iter_mut() {
			if let Some(task) = task.take() {
				let _ = task.send(());
			}
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

impl<M: core::Metadata, S: core::Middleware<M>> ws::Handler for Session<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
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

		self.context.origin = origin
			.and_then(|origin| ::std::str::from_utf8(origin).ok())
			.map(Into::into);
		self.context.protocols = req
			.protocols()
			.ok()
			.map(|protos| protos.into_iter().map(Into::into).collect())
			.unwrap_or_else(Vec::new);
		self.metadata = Some(self.meta_extractor.extract(&self.context));

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
		let metadata = self
			.metadata
			.clone()
			.expect("Metadata is always set in on_request; qed");

		// TODO: creation requires allocating a `oneshot` channel and acquiring a
		// mutex. we could alternatively do this lazily upon first poll if
		// it becomes a bottleneck.
		let poll_liveness = LivenessPoll::create(self.task_slab.clone());

		let active_lock = self.active.clone();
		let response = self.handler.handle_request(req, metadata);

		let future = response.map(move |response| {
			if !active_lock.load(atomic::Ordering::Relaxed) {
				return;
			}
			if let Some(result) = response {
				let res = out.send(result);
				match res {
					Err(error::Error::ConnectionClosed) => {
						active_lock.store(false, atomic::Ordering::Relaxed);
					}
					Err(e) => {
						warn!("Error while sending response: {:?}", e);
					}
					_ => {}
				}
			}
		});

		let future = future::select(future, poll_liveness);
		self.executor.spawn(future);

		Ok(())
	}
}

pub struct Factory<M: core::Metadata, S: core::Middleware<M>> {
	session_id: SessionId,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<dyn metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	allowed_hosts: Option<Vec<Host>>,
	request_middleware: Option<Arc<dyn RequestMiddleware>>,
	stats: Option<Arc<dyn SessionStats>>,
	executor: TaskExecutor,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<dyn metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<Origin>>,
		allowed_hosts: Option<Vec<Host>>,
		request_middleware: Option<Arc<dyn RequestMiddleware>>,
		stats: Option<Arc<dyn SessionStats>>,
		executor: TaskExecutor,
	) -> Self {
		Factory {
			session_id: 0,
			handler,
			meta_extractor,
			allowed_origins,
			allowed_hosts,
			request_middleware,
			stats,
			executor,
		}
	}
}

impl<M: core::Metadata, S: core::Middleware<M>> ws::Factory for Factory<M, S>
where
	S::Future: Unpin,
	S::CallFuture: Unpin,
{
	type Handler = Session<M, S>;

	fn connection_made(&mut self, sender: ws::Sender) -> Self::Handler {
		self.session_id += 1;
		if let Some(executor) = self.stats.as_ref() {
			executor.open_session(self.session_id)
		}
		let active = Arc::new(atomic::AtomicBool::new(true));

		Session {
			active: active.clone(),
			context: metadata::RequestContext {
				session_id: self.session_id,
				origin: None,
				protocols: Vec::new(),
				out: metadata::Sender::new(sender, active),
				executor: self.executor.clone(),
			},
			handler: self.handler.clone(),
			meta_extractor: self.meta_extractor.clone(),
			allowed_origins: self.allowed_origins.clone(),
			allowed_hosts: self.allowed_hosts.clone(),
			stats: self.stats.clone(),
			request_middleware: self.request_middleware.clone(),
			metadata: None,
			executor: self.executor.clone(),
			task_slab: Arc::new(Mutex::new(Slab::with_capacity(0))),
		}
	}
}

fn header_is_allowed<T>(allowed: &Option<Vec<T>>, header: Option<&[u8]>) -> bool
where
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
					return true;
				}
			}
			false
		}
		// Disallow in other cases
		_ => false,
	}
}

fn forbidden(title: &str, message: &str) -> ws::Response {
	let mut forbidden = ws::Response::new(403, "Forbidden", format!("{}\n{}\n", title, message).into_bytes());
	{
		let headers = forbidden.headers_mut();
		headers.push(("Connection".to_owned(), b"close".to_vec()));
	}
	forbidden
}
