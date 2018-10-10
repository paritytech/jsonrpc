use std::time::{Duration};
use tokio_timer::{wheel, Timer, Sleep};
use std::io;
use core::futures::prelude::*;
use core::futures::task;
/// `Incoming` is a stream of incoming sockets
/// Polling the stream may return a temporary io::Error
/// (for instance if we can't open the connection because of "too many open files" limit)
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
	timeout: Option<Sleep>,
	timer: Timer
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
			timer: wheel().build()
		}
	}
}

impl<S, I> Stream for SuspendableStream<S>
	where S: Stream<Item=I, Error=io::Error>
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
						println!("Timeout error {:?}", err);
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
					return Ok(item)
				}
				Err(ref err) => {
					if connection_error(err) {
						warn!("Connection Error: {:?}", err);
						continue
					}
					self.next_delay = if self.next_delay < self.max_delay {
						self.next_delay * 2
					} else {
						self.next_delay
					};
					warn!("Error accepting connection: {}", err);
					warn!("The server will stop accepting connections for {:?}", self.next_delay);
					self.timeout = Some(self.timer.sleep(self.next_delay));
				}
			}
		}
	}
}


/// assert that the error was a connection error
fn connection_error(e: &io::Error) -> bool {
	e.kind() == io::ErrorKind::ConnectionRefused ||
		e.kind() == io::ErrorKind::ConnectionAborted ||
		e.kind() == io::ErrorKind::ConnectionReset
}

#[cfg(test)]
mod test {
	use super::*;
	use core::futures::future::poll_fn;
	struct TestStream {
		items: Vec<Poll<Option<u8>, io::Error>>
	}

	impl Stream for TestStream {
		type Item = u8;
		type Error = io::Error;

		fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
			if let Some(item) = self.items.pop() {
				task::current().notify();
				return item
			}
			return Ok(Async::Ready(None))
		}
	}

	#[test]
	fn test_suspendable_stream() {
		// this test encompasses all the properties of SuspendableStream
		// 1. tests that the timeout is set in the event of an error
		// 2. the `next_delay` increases in the order of *= 2 for every error encountered.
		poll_fn(|| {
			let items = {
				let mut i = vec![
					Ok(Async::Ready(Some(1))),
					Err(io::ErrorKind::Other.into()),
					Ok(Async::Ready(Some(2))),
					Ok(Async::Ready(Some(3))),
					Err(io::ErrorKind::Other.into()),
					Ok(Async::Ready(Some(4))),
					Err(io::ErrorKind::Other.into()),
					Err(io::ErrorKind::Other.into()),
					Err(io::ErrorKind::Other.into()),
					Ok(Async::Ready(Some(5))),
					Err(io::ErrorKind::Other.into()),
					Ok(Async::Ready(Some(6))),
				];
				i.reverse();
				i
			};

			let mut stream = SuspendableStream::new(TestStream { items });

			let items = vec![1, 2, 3, 4, 5, 6];
			let mut stream_items = vec![];

			let mut error_count = 1;
			let duration = Duration::from_millis(20);
			loop {
				match stream.poll() {
					Ok(Async::Ready(Some(item))) => {
						error_count = 1;
						stream_items.push(item)
					},
					Ok(Async::Ready(None)) => {
						break
					}
					Ok(Async::NotReady) => return Ok(Async::NotReady),
					Err(_) => {
						error_count *= 2;
						assert_eq!(stream.timeout.is_some(), true);
						assert_eq!(stream.next_delay, error_count * duration)
					},
				}
			}

			assert_eq!(stream_items, items);
			Ok::<_, ()>(Async::Ready(()))
		}).wait().unwrap();
	}
}
