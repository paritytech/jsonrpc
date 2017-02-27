use std;
use std::sync::Arc;

use core;
use core::futures::Future;
use ws;

use metadata;

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
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
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
		let context = metadata::RequestContext {
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
	meta_extractor: Arc<metadata::MetaExtractor<M>>,
	allowed_origins: Option<Vec<String>>,
}

impl<M: core::Metadata, S: core::Middleware<M>> Factory<M, S> {
	pub fn new(
		handler: Arc<core::MetaIoHandler<M, S>>,
		meta_extractor: Arc<metadata::MetaExtractor<M>>,
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
