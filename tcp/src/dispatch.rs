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

use futures::{task, Stream, Poll, Async};

use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::Entry;


pub type MessageQueue = Mutex<HashMap<SocketAddr, VecDeque<String>>>;
pub type TaskNotificationQueue = Mutex<Vec<task::Task>>;

pub struct PeerMessageQueue<S: Stream> {
    up: S,
    queue: Arc<MessageQueue>,
    task: Arc<TaskNotificationQueue>,
    addr: SocketAddr,
}

impl<S: Stream> PeerMessageQueue<S> {
    pub fn new(response_stream: S,
        message_queue: Arc<MessageQueue>,
        task_notifications: Arc<TaskNotificationQueue>,
        addr: SocketAddr,
    ) -> Self {
        PeerMessageQueue {
            up: response_stream,
            queue: message_queue,
            task: task_notifications,
            addr: addr,
        }
    }
}

pub struct Dispatcher {
    message_queue: Arc<MessageQueue>,
    task: Arc<TaskNotificationQueue>,
}

impl Dispatcher {
    pub fn new(message_queue: Arc<MessageQueue>, task_notifications: Arc<TaskNotificationQueue>) -> Self {
        Dispatcher {
            message_queue: message_queue,
            task: task_notifications,
        }
    }

    pub fn push_message(&self, peer_addr: &SocketAddr, msg: String) {
        let mut queue = self.message_queue.lock().unwrap();
        match queue.entry(peer_addr.clone()) {
            Entry::Vacant(vacant) => {
                let mut deque = VecDeque::new();
                deque.push_back(msg);
                vacant.insert(deque);
            },
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().push_back(msg);
            }
        };

        let mut task_lock = self.task.lock().unwrap();
        let tasks = task_lock.drain(..).collect::<Vec<task::Task>>();
        for task in tasks { task.unpark(); }
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
            _ => {}
        }

        // then try to send queued message
        let mut queue = match self.queue.try_lock() {
            Ok(lock) => lock,
            Err(_) => {
                self.task.lock().unwrap().push(task::park());
                return Ok(Async::NotReady);
            }
        };

        match queue.get_mut(&self.addr) {
            None => {
                self.task.lock().unwrap().push(task::park());
                return Ok(Async::NotReady)
            },
            Some(mut peer_deque) => {
                match peer_deque.pop_front() {
                    None => {
                        self.task.lock().unwrap().push(task::park());
                        return Ok(Async::NotReady);
                    },
                    Some(msg) => {
                        Ok(Async::Ready(Some(msg)))
                    }
                }
            }
        }
    }
}
