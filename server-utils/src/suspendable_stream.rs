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

impl<S, I> SuspendableStream<S>
	where S: Stream<Item=I, Error=io::Error>
{
	/// construct a new Suspendable stream, given tokio::Incoming
	/// and the amount of time to pause for.
	pub fn new(stream: S, delay: Duration) -> Self {
		SuspendableStream {
			stream,
			delay,
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
		if let Some(ref mut timeout) = self.timeout {
			match timeout.poll() {
				Ok(Async::Ready(_)) => {}
				Ok(Async::NotReady) => return Ok(Async::NotReady),
				Err(_) => unreachable!("Polling a delay shouldn't yield any errors")
			}
		}

		self.timeout = None;

		loop {
			match self.stream.poll() {
				Ok(socket) => return Ok(socket),
				Err(err) => {
					//  Os { code: 24, kind: Other, message: "Too many open files" }
					if let Some(24) = err.raw_os_error() {
						debug!("Error accepting connection: {}", err);
						debug!("The server will stop accepting connections for {:?}", self.delay);
						let mut timeout = Delay::new(Instant::now() + self.delay);

						match timeout.poll() {
							Ok(Async::Ready(())) => continue,
							Ok(Async::NotReady) => {
								self.timeout = Some(timeout);
								return Ok(Async::NotReady);
							}
							Err(_) => unreachable!("Polling a delay shouldn't yield any errors")
						}
					} else {
						continue
					}
				}
			}
		}
	}
}
