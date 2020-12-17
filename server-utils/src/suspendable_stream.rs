use std::io;
use std::time::Duration;
// use futures01::prelude::*;
use tokio02::time::Delay;

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

// impl<S, I> futures01::Stream for SuspendableStream<S>
// where
// 	S: futures01::Stream<Item = I, Error = io::Error>,
// {
// 	type Item = I;
// 	type Error = ();

// 	fn poll(&mut self) -> Result<Async<Option<Self::Item>>, ()> {
// 		loop {
// 			if let Some(timeout) = self.timeout.take() {
// 				let deadline = timeout.deadline();
// 				use futures03::FutureExt;
// 				let mut compat = futures03::compat::Compat::new(timeout.map(Ok::<(), ()>));
// 				match futures01::Future::poll(&mut compat) {
// 					Ok(Async::Ready(_)) => {}
// 					Ok(Async::NotReady) => {
// 						self.timeout = Some(tokio02::time::delay_until(deadline));
// 						return Ok(Async::NotReady);
// 					}
// 					Err(err) => {
// 						warn!("Timeout error {:?}", err);
// 						futures01::task::current().notify();
// 						return Ok(Async::NotReady);
// 					}
// 				}
// 			}

// 			match self.stream.poll() {
// 				Ok(item) => {
// 					if self.next_delay > self.initial_delay {
// 						self.next_delay = self.initial_delay;
// 					}
// 					return Ok(item);
// 				}
// 				Err(ref err) => {
// 					if connection_error(err) {
// 						warn!("Connection Error: {:?}", err);
// 						continue;
// 					}
// 					self.next_delay = if self.next_delay < self.max_delay {
// 						self.next_delay * 2
// 					} else {
// 						self.next_delay
// 					};
// 					debug!("Error accepting connection: {}", err);
// 					debug!("The server will stop accepting connections for {:?}", self.next_delay);
// 					self.timeout = Some(tokio02::time::delay_for(self.next_delay));
// 				}
// 			}
// 		}
// 	}
// }

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

impl<S, I> futures03::Stream for SuspendableStream<S>
where
	S: futures03::Stream<Item = io::Result<I>> + Unpin,
{
	type Item = I;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
		loop {
			if let Some(timeout) = self.timeout.as_mut() {
				match Pin::new(timeout).poll(cx) {
					Poll::Pending => return Poll::Pending,
					Poll::Ready(()) => {}
				}
			}

			match Pin::new(&mut self.stream).poll_next(cx) {
				Poll::Pending => {
					if self.next_delay > self.initial_delay {
						self.next_delay = self.initial_delay;
					}
					return Poll::Pending;
				},
				Poll::Ready(None) => {
					if self.next_delay > self.initial_delay {
						self.next_delay = self.initial_delay;
					}
					return Poll::Ready(None);
				}
				Poll::Ready(Some(Ok(item))) => {
					if self.next_delay > self.initial_delay {
						self.next_delay = self.initial_delay;
					}

					return Poll::Ready(Some(item));
				}
				Poll::Ready(Some(Err(ref err))) => {
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
					self.timeout = Some(tokio02::time::delay_for(self.next_delay));
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
