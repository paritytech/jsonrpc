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

impl<S> SuspendableStream<S> {
	/// construct a new Suspendable stream, given tokio::Incoming
	/// and the amount of time to pause for.
	pub fn new(stream: S) -> Self {
		SuspendableStream {
			stream,
			delay: Duration::from_millis(500),
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
		let mut recovered = None;
		if let Some(mut timeout) = self.timeout.take() {
			match timeout.poll() {
				Ok(Async::Ready(_)) => {
					recovered = Some(());
				}
				Ok(Async::NotReady) => {
					self.timeout = Some(timeout);
					return Ok(Async::NotReady)
				},
				Err(_) => unreachable!("Polling a delay shouldn't yield any errors")
			}
		}

		loop {
			match self.stream.poll() {
				Ok(item) => return Ok(item),
				Err(err) => {
					self.delay = if recovered.is_some() && self.delay.as_secs() < 5 {
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
