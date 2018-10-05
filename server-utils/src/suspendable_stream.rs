use std::time::{Duration, Instant};
use tokio::timer::Delay;
use std::io;
use tokio::prelude::*;

/// A wrapper type for tokio::Incoming that stops accepting new connections
/// for a specified amount of time once `ulimit` is reached
pub struct SuspendableStream<S> {
	stream: S,
	delay: Duration,
	timeout: Option<Delay>,
}

const MAX_DELAY_SECS: u64 = 5;

impl<S> SuspendableStream<S> {
	/// construct a new Suspendable stream, given tokio::Incoming
	/// and the amount of time to pause for.
	pub fn new(stream: S) -> Self {
		SuspendableStream {
			stream,
			delay: Duration::from_millis(20),
			timeout: None,
		}
	}
}

impl<S, I> Stream for SuspendableStream<S>
	where S: Stream<Item=I, Error=io::Error>
{
	type Item = I;
	type Error = ();

	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, ()> {
		let mut resumed = false;
		if let Some(mut timeout) = self.timeout.take() {
			match timeout.poll() {
				Ok(Async::Ready(_)) => {
					resumed = true;
				}
				Ok(Async::NotReady) => {
					self.timeout = Some(timeout);
					return Ok(Async::NotReady);
				}
				Err(_) => unreachable!("Polling a delay shouldn't yield any errors; qed")
			}
		}

		loop {
			match self.stream.poll() {
				Ok(item) => {
					if self.delay.as_millis() > 20 {
						self.delay = Duration::from_millis(20);
					}
					return Ok(item)
				},
				Err(ref e) => if connection_error(e) {
					warn!("Connection Error: {:?}", e);
					continue
				}
				Err(err) => {
					self.delay = if resumed && self.delay.as_secs() < MAX_DELAY_SECS {
						self.delay * 2
					} else {
						self.delay
					};
					warn!("Error accepting connection: {}", err);
					warn!("The server will stop accepting connections for {:?}", self.delay);
					self.timeout = Some(Delay::new(Instant::now() + self.delay));
				}
			}
		}
	}
}

fn connection_error(e: &io::Error) -> bool {
	e.kind() == io::ErrorKind::ConnectionRefused ||
		e.kind() == io::ErrorKind::ConnectionAborted ||
		e.kind() == io::ErrorKind::ConnectionReset
}

