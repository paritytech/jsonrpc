use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{atomic, Arc};
use std::task::{Context, Poll};

use crate::core;
use crate::core::futures::channel::mpsc;
use crate::server_utils::{reactor::TaskExecutor, session};
use crate::ws;

use crate::error;
use crate::Origin;

/// Output of WebSocket connection. Use this to send messages to the other endpoint.
#[derive(Clone)]
pub struct Sender {
	out: ws::Sender,
	active: Arc<atomic::AtomicBool>,
}

impl Sender {
	/// Creates a new `Sender`.
	pub fn new(out: ws::Sender, active: Arc<atomic::AtomicBool>) -> Self {
		Sender { out, active }
	}

	fn check_active(&self) -> error::Result<()> {
		if self.active.load(atomic::Ordering::SeqCst) {
			Ok(())
		} else {
			Err(error::Error::ConnectionClosed)
		}
	}

	/// Sends a message over the connection.
	/// Will return error if the connection is not active any more.
	pub fn send<M>(&self, msg: M) -> error::Result<()>
	where
		M: Into<ws::Message>,
	{
		self.check_active()?;
		self.out.send(msg)?;
		Ok(())
	}

	/// Sends a message over the endpoints of all connections.
	/// Will return error if the connection is not active any more.
	pub fn broadcast<M>(&self, msg: M) -> error::Result<()>
	where
		M: Into<ws::Message>,
	{
		self.check_active()?;
		self.out.broadcast(msg)?;
		Ok(())
	}

	/// Sends a close code to the other endpoint.
	/// Will return error if the connection is not active any more.
	pub fn close(&self, code: ws::CloseCode) -> error::Result<()> {
		self.check_active()?;
		self.out.close(code)?;
		Ok(())
	}
}

/// Request context
pub struct RequestContext {
	/// Session id
	pub session_id: session::SessionId,
	/// Request Origin
	pub origin: Option<Origin>,
	/// Requested protocols
	pub protocols: Vec<String>,
	/// Direct channel to send messages to a client.
	pub out: Sender,
	/// Remote to underlying event loop.
	pub executor: TaskExecutor,
}

impl RequestContext {
	/// Get this session as a `Sink` spawning a new future
	/// in the underlying event loop.
	pub fn sender(&self) -> mpsc::UnboundedSender<String> {
		let out = self.out.clone();
		let (sender, receiver) = mpsc::unbounded();
		self.executor.spawn(SenderFuture(out, Box::new(receiver)));
		sender
	}
}

impl fmt::Debug for RequestContext {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		fmt.debug_struct("RequestContext")
			.field("session_id", &self.session_id)
			.field("origin", &self.origin)
			.field("protocols", &self.protocols)
			.finish()
	}
}

/// Metadata extractor from session data.
pub trait MetaExtractor<M: core::Metadata>: Send + Sync + 'static {
	/// Extract metadata for given session
	fn extract(&self, _context: &RequestContext) -> M;
}

impl<M, F> MetaExtractor<M> for F
where
	M: core::Metadata,
	F: Fn(&RequestContext) -> M + Send + Sync + 'static,
{
	fn extract(&self, context: &RequestContext) -> M {
		(*self)(context)
	}
}

/// Dummy metadata extractor
#[derive(Debug, Clone)]
pub struct NoopExtractor;
impl<M: core::Metadata + Default> MetaExtractor<M> for NoopExtractor {
	fn extract(&self, _context: &RequestContext) -> M {
		M::default()
	}
}

struct SenderFuture(Sender, Box<dyn futures::Stream<Item = String> + Send + Unpin>);

impl Future for SenderFuture {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		use futures::Stream;

		let this = Pin::into_inner(self);
		loop {
			match Pin::new(&mut this.1).poll_next(cx) {
				Poll::Pending => return Poll::Pending,
				Poll::Ready(None) => return Poll::Ready(()),
				Poll::Ready(Some(val)) => {
					if let Err(e) = this.0.send(val) {
						warn!("Error sending a subscription update: {:?}", e);
						return Poll::Ready(());
					}
				}
			}
		}
	}
}
