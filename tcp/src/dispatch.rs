use std;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc};

use jsonrpc::futures::{Stream, Poll, Async, Sink, Future};
use jsonrpc::futures::sync::mpsc;

use parking_lot::Mutex;

pub type SenderChannels = Mutex<HashMap<SocketAddr, mpsc::Sender<String>>>;

pub struct PeerMessageQueue<S: Stream> {
	up: S,
	receiver: mpsc::Receiver<String>,
	_addr: SocketAddr,
}

impl<S: Stream> PeerMessageQueue<S> {
	pub fn new(
		response_stream: S,
		receiver: mpsc::Receiver<String>,
		addr: SocketAddr,
	) -> Self {
		PeerMessageQueue {
			up: response_stream,
			receiver: receiver,
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
	Send(mpsc::SendError<String>)
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
		Dispatcher {
			channels: channels,
		}
	}

	/// Pushes message to given peer
	pub fn push_message(&self, peer_addr: &SocketAddr, msg: String) -> Result<(), PushMessageError> {
		let mut channels = self.channels.lock();

		match channels.get_mut(peer_addr) {
			Some(channel) => {
				// todo: maybe async here later?
				try!(channel.send(msg).wait().map_err(|e| PushMessageError::from(e)));
				Ok(())
			},
			None => {
				return Err(PushMessageError::NoSuchPeer);
			}
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

impl<S: Stream<Item=String, Error=std::io::Error>> Stream for PeerMessageQueue<S> {

	type Item = String;
	type Error = std::io::Error;

	fn poll(&mut self) -> Poll<Option<String>, std::io::Error> {
		// check if we have response pending
		match self.up.poll() {
			Ok(Async::Ready(Some(val))) => {
				return Ok(Async::Ready(Some(val)));
			},
			Ok(Async::Ready(None)) => {
				// this will ensure that this polling will end when incoming i/o stream ends
				return Ok(Async::Ready(None));
			},
			_ => {}
		}

		match self.receiver.poll() {
			Ok(result) => Ok(result),
			Err(send_err) => {
				// not sure if it can ever happen
				warn!("MPSC send error: {:?}", send_err);
				Err(std::io::Error::from(std::io::ErrorKind::Other))
			}
		}
	}
}
