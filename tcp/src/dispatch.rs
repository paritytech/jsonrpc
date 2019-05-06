use std;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::jsonrpc::futures::sync::mpsc;
use crate::jsonrpc::futures::{Async, Future, Poll, Sink, Stream};

use parking_lot::Mutex;

pub type SenderChannels = Mutex<HashMap<SocketAddr, mpsc::Sender<String>>>;

pub struct PeerMessageQueue<S: Stream> {
	up: S,
	receiver: Option<mpsc::Receiver<String>>,
	_addr: SocketAddr,
}

impl<S: Stream> PeerMessageQueue<S> {
	pub fn new(response_stream: S, receiver: mpsc::Receiver<String>, addr: SocketAddr) -> Self {
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
	Send(mpsc::SendError<String>),
}

impl From<mpsc::SendError<String>> for PushMessageError {
	fn from(send_err: mpsc::SendError<String>) -> Self {
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
				// todo: maybe async here later?
				channel.send(msg).wait().map_err(PushMessageError::from)?;
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

impl<S: Stream<Item = String, Error = std::io::Error>> Stream for PeerMessageQueue<S> {
	type Item = String;
	type Error = std::io::Error;

	// The receiver will never return `Ok(Async::Ready(None))`
	// Because the sender is kept in `SenderChannels` and it will never be dropped until `the stream` is resolved.
	//
	// Thus, that is the reason we terminate if `up_closed && receiver == Async::NotReady`.
	//
	// However, it is possible to have a race between `poll` and `push_work` if the connection is dropped.
	// Therefore, the receiver is then dropped when the connection is dropped and an error is propagated when
	// a `send` attempt is made on that channel.
	fn poll(&mut self) -> Poll<Option<String>, std::io::Error> {
		// check if we have response pending

		let up_closed = match self.up.poll() {
			Ok(Async::Ready(Some(item))) => return Ok(Async::Ready(Some(item))),
			Ok(Async::Ready(None)) => true,
			Ok(Async::NotReady) => false,
			err => return err,
		};

		let rx = match &mut self.receiver {
			None => {
				debug_assert!(up_closed);
				return Ok(Async::Ready(None));
			}
			Some(rx) => rx,
		};

		match rx.poll() {
			Ok(Async::Ready(Some(item))) => Ok(Async::Ready(Some(item))),
			Ok(Async::Ready(None)) | Ok(Async::NotReady) if up_closed => {
				self.receiver = None;
				Ok(Async::Ready(None))
			}
			Ok(Async::Ready(None)) | Ok(Async::NotReady) => Ok(Async::NotReady),
			Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "MPSC error")),
		}
	}
}
