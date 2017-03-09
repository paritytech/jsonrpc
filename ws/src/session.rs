use std;
use std::sync::Arc;

use core;
use core::futures::Future;
use server_utils::tokio_core::reactor::Remote;
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

pub struct Session<M: core::Metadata, S: core::Middleware<M>> {
	context: metadata::RequestContext,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	metadata: M,
	remote: Remote,
}

impl<M: core::Metadata, S: core::Middleware<M>> Drop for Session<M, S> {
	fn drop(&mut self) {
		self.stats.as_ref().map(|stats| stats.close_session(self.context.session_id));
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

		let future = self.handler.handle_request(req, metadata)
			.map(move |response| {
				if let Some(result) = response {
					let res = out.send(result);
					if let Err(e) = res {
						warn!(target: "signer", "Error while sending response: {:?}", e);
					}
				}
			});
		self.remote.spawn(|_| future);

		Ok(())
	}
}

pub struct Factory<M: core::Metadata, S: core::Middleware<M>> {
	session_id: SessionId,
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
	request_middleware: Option<Arc<RequestMiddleware>>,
	stats: Option<Arc<SessionStats>>,
	remote: Remote,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<metadata::MetaExtractor<M>>,
		allowed_origins: Option<Vec<String>>,
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
		}
	}
}

fn origin_is_allowed(allowed_origins: &Option<Vec<String>>, header: Option<&[u8]>) -> bool {
	let header = header.map(std::str::from_utf8);

	match (header, allowed_origins.as_ref()) {
		// Always allow if Origin is not specified
		(None, _) => true,
		// Always allow if Origin validation is disabled
		(_, None) => true,
		// Validate Origin
		(Some(Ok(origin)), Some(origins)) => {
			for o in origins {
				if o == origin {
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
