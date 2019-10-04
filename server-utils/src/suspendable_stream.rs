use std::io;
use std::time::{Duration, Instant};
use tokio::prelude::*;
use tokio::timer::Delay;

/// `Incoming` is a stream of incoming sockets
/// Polling the stream may return a temporary io::Error (for instance if we can't open the connection because of "too many open files" limit)
/// we use for_each combinator which:
/// 1. Runs for every Ok(socket)
/// 2. Stops on the FIRST Err()
/// So any temporary io::Error will cause the entire server to terminate.
/// This wrapper type for tokio::Incoming stops accepting new connections
/// for a specified amount of time once an io::Error is encountered
pub struct SuspendableStream<S> {
	stream: S,
	next_delay: Duration,
	initial_delay: Duration,
	max_delay: Duration,
	timeout: Option<Delay>,
}

impl<S> SuspendableStream<S> {
	/// construct a new Suspendable stream, given tokio::Incoming
	/// and the amount of time to pause for.
	pub fn new(stream: S) -> Self {
		SuspendableStream {
			stream,
			next_delay: Duration::from_millis(20),
			initial_delay: Duration::from_millis(10),
			max_delay: Duration::from_secs(5),
			timeout: None,
		}
	}
}

impl<S, I> Stream for SuspendableStream<S>
where
	S: Stream<Item = I, Error = io::Error>,
{
	type Item = I;
	type Error = ();

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, ()> {
		loop {
			if let Some(mut timeout) = self.timeout.take() {
				match timeout.poll() {
					Ok(Async::Ready(_)) => {}
					Ok(Async::NotReady) => {
						self.timeout = Some(timeout);
						return Ok(Async::NotReady);
					}
					Err(err) => {
						warn!("Timeout error {:?}", err);
						task::current().notify();
						return Ok(Async::NotReady);
					}
				}
			}

			match self.stream.poll() {
				Ok(item) => {
					if self.next_delay > self.initial_delay {
						self.next_delay = self.initial_delay;
					}
					return Ok(item);
				}
				Err(ref err) => {
					if connection_error(err) {
						warn!("Connection Error: {:?}", err);
						continue;
					}
					self.next_delay = if self.next_delay < self.max_delay {
						self.next_delay * 2
					} else {
						self.next_delay
					};
					debug!("Error accepting connection: {}", err);
					debug!("The server will stop accepting connections for {:?}", self.next_delay);
					self.timeout = Some(Delay::new(Instant::now() + self.next_delay));
				}
			}
		}
	}
}

/// assert that the error was a connection error
fn connection_error(e: &io::Error) -> bool {
	e.kind() == io::ErrorKind::ConnectionRefused
		|| e.kind() == io::ErrorKind::ConnectionAborted
		|| e.kind() == io::ErrorKind::ConnectionReset
}
