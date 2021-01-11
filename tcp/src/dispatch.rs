use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use crate::futures::{channel::mpsc, Stream};

use parking_lot::Mutex;

pub type SenderChannels = Mutex<HashMap<SocketAddr, mpsc::UnboundedSender<String>>>;

pub struct PeerMessageQueue<S: Stream + Unpin> {
	up: S,
	receiver: Option<mpsc::UnboundedReceiver<String>>,
	_addr: SocketAddr,
}

impl<S: Stream + Unpin> PeerMessageQueue<S> {
	pub fn new(response_stream: S, receiver: mpsc::UnboundedReceiver<String>, addr: SocketAddr) -> Self {
		PeerMessageQueue {
			up: response_stream,
			receiver: Some(receiver),
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

impl<S: Stream<Item = std::io::Result<String>> + Unpin> Stream for PeerMessageQueue<S> {
	type Item = std::io::Result<String>;

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
			Poll::Ready(Some(Ok(item))) => return Poll::Ready(Some(Ok(item))),
			Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
			Poll::Ready(None) => true,
			Poll::Pending => false,
		};

		let mut rx = match &mut this.receiver {
			None => {
				debug_assert!(up_closed);
				return Poll::Ready(None);
			}
			Some(rx) => rx,
		};

		match Pin::new(&mut rx).poll_next(cx) {
			Poll::Ready(Some(item)) => Poll::Ready(Some(Ok(item))),
			Poll::Ready(None) | Poll::Pending if up_closed => {
				this.receiver = None;
				Poll::Ready(None)
			}
			Poll::Ready(None) | Poll::Pending => Poll::Pending,
		}
	}
}
