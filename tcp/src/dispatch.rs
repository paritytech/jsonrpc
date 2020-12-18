use std;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::jsonrpc::futures::{self as futures03, channel::mpsc, StreamExt};
// use futures01::{Async,
	// Poll,
	// Stream
// };
use futures03::Stream;
use std::task::Poll;

use parking_lot::Mutex;

pub type SenderChannels = Mutex<HashMap<SocketAddr, mpsc::UnboundedSender<String>>>;

pub struct PeerMessageQueue<S: Stream + Unpin> {
	up: S,
	receiver: Option<mpsc::UnboundedReceiver<String>>,
	// receiver: Option<Box<dyn Stream<Item = String> + Send>>,
	_addr: SocketAddr,
}

impl<S: Stream + Unpin> PeerMessageQueue<S> {
	pub fn new(response_stream: S, receiver: mpsc::UnboundedReceiver<String>, addr: SocketAddr) -> Self {
		// let receiver = futures03::compat::Compat::new(receiver.map(|v| Ok(v)));
		PeerMessageQueue {
			up: response_stream,
			receiver: Some(receiver),
			// receiver: Some(Box::new(receiver)),
			_addr: addr,
		}
	}
}

/// Push Message Error
#[derive(Debug)]
pub enum PushMessageError {
	/// Invalid peer
	NoSuchPeer,
	/// Send error
	Send(mpsc::TrySendError<String>),
}

impl From<mpsc::TrySendError<String>> for PushMessageError {
	fn from(send_err: mpsc::TrySendError<String>) -> Self {
		PushMessageError::Send(send_err)
	}
}

/// Peer-messages dispatcher.
#[derive(Clone)]
pub struct Dispatcher {
	channels: Arc<SenderChannels>,
}

impl Dispatcher {
	/// Creates a new dispatcher
	pub fn new(channels: Arc<SenderChannels>) -> Self {
		Dispatcher { channels }
	}

	/// Pushes message to given peer
	pub fn push_message(&self, peer_addr: &SocketAddr, msg: String) -> Result<(), PushMessageError> {
		let mut channels = self.channels.lock();

		match channels.get_mut(peer_addr) {
			Some(channel) => {
				channel.unbounded_send(msg).map_err(PushMessageError::from)?;
				Ok(())
			}
			None => Err(PushMessageError::NoSuchPeer),
		}
	}

	/// Returns `true` if the peer is still connnected
	pub fn is_connected(&self, socket_addr: &SocketAddr) -> bool {
		self.channels.lock().contains_key(socket_addr)
	}

	/// Returns current peer count.
	pub fn peer_count(&self) -> usize {
		self.channels.lock().len()
	}
}

use std::pin::Pin;

impl<S: Unpin + Stream<Item = std::io::Result<String>>> Stream for PeerMessageQueue<S> {
	type Item = std::io::Result<String>;
	// type Error = std::io::Error;

	// The receiver will never return `Ok(Async::Ready(None))`
	// Because the sender is kept in `SenderChannels` and it will never be dropped until `the stream` is resolved.
	//
	// Thus, that is the reason we terminate if `up_closed && receiver == Async::NotReady`.
	//
	// However, it is possible to have a race between `poll` and `push_work` if the connection is dropped.
	// Therefore, the receiver is then dropped when the connection is dropped and an error is propagated when
	// a `send` attempt is made on that channel.
	fn poll_next(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
		// check if we have response pending
		let this = Pin::into_inner(self);

		let up_closed = match Pin::new(&mut this.up).poll_next(cx) {
			Poll::Pending => false,
			Poll::Ready(None) => true,
			Poll::Ready(Some(Ok(item))) => return Poll::Ready(Some(Ok(item))),
			Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
		};
		// 	Ok(Async::Ready(Some(item))) => return Ok(Async::Ready(Some(item))),
		// 	Ok(Async::Ready(None)) => true,
		// 	Ok(Async::NotReady) => false,
		// 	err => return err,
		// };

		let mut rx = match &mut this.receiver {
			None => {
				debug_assert!(up_closed);
				return Poll::Ready(None);
			}
			Some(rx) => rx,
		};

		match Pin::new(&mut rx).poll_next(cx) {
			Poll::Ready(Some(item)) => Poll::Ready(Some(Ok(item))),
			Poll::Pending | Poll::Ready(None) if up_closed => {
				this.receiver = None;
				Poll::Ready(None)
			}
			Poll::Pending | Poll::Ready(None) => Poll::Pending,
		}
		// match rx.poll() {
		// 	Ok(Async::Ready(Some(item))) => Ok(Async::Ready(Some(item))),
		// 	Ok(Async::Ready(None)) | Ok(Async::NotReady) if up_closed => {
		// 		self.receiver = None;
		// 		Ok(Async::Ready(None))
		// 	}
		// 	Ok(Async::Ready(None)) | Ok(Async::NotReady) => Ok(Async::NotReady),
		// 	Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "MPSC error")),
		// }
	}
}
