// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

use std;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures::{Stream, Poll, Async, Sink, Future};
use futures::sync::mpsc;

use std::collections::HashMap;

pub type SenderChannels = Mutex<HashMap<SocketAddr, mpsc::Sender<String>>>;

pub struct PeerMessageQueue<S: Stream> {
    up: S,
    receiver: mpsc::Receiver<String>,
    _addr: SocketAddr,
}

impl<S: Stream> PeerMessageQueue<S> {
    pub fn new(response_stream: S,
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

#[derive(Debug)]
pub enum PushMessageError { NoSuchPeer, Send(mpsc::SendError<String>) }

impl From<mpsc::SendError<String>> for PushMessageError {
    fn from(send_err: mpsc::SendError<String>) -> Self {
        PushMessageError::Send(send_err)
    }
}

pub struct Dispatcher {
    channels: Arc<SenderChannels>,
}

impl Dispatcher {
    pub fn new(channels: Arc<SenderChannels>) -> Self {
        Dispatcher {
            channels: channels,
        }
    }

    pub fn push_message(&self, peer_addr: &SocketAddr, msg: String) -> Result<(), PushMessageError> {
        let mut channels = self.channels.lock().unwrap();

        match channels.get_mut(peer_addr) {
            Some(mut channel) => {
                // todo: maybe async here later?
                try!(channel.send(msg).wait().map_err(|e| PushMessageError::from(e)));
                Ok(())
            },
            None => {
                return Err(PushMessageError::NoSuchPeer);
            }
        }
    }

    pub fn is_connected(&self, socket_addr: &SocketAddr) -> bool {
        self.channels.lock().unwrap().contains_key(socket_addr)
    }

    pub fn peer_count(&self) -> usize {
        self.channels.lock().unwrap().len()
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
