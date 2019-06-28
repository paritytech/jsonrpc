//! Duplex transport

use failure::format_err;
use futures::prelude::*;
use futures::sync::{mpsc, oneshot};
use jsonrpc_core::Id;
use jsonrpc_pubsub::SubscriptionId;
use log::debug;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::VecDeque;

use super::RequestBuilder;
use crate::{RpcChannel, RpcError, RpcMessage};

struct Subscription {
	/// Subscription id received when subscribing.
	id: Option<SubscriptionId>,
	/// Rpc method to unsubscribe.
	unsubscribe: String,
	/// Where to send messages to.
	channel: mpsc::Sender<Result<Value, RpcError>>,
}

impl Subscription {
	fn new(channel: mpsc::Sender<Result<Value, RpcError>>, unsubscribe: String) -> Self {
		Subscription {
			id: None,
			unsubscribe,
			channel,
		}
	}
}

/// The Duplex handles sending and receiving asynchronous
/// messages through an underlying transport.
pub struct Duplex<TSink, TStream> {
	request_builder: RequestBuilder,
	/// Channel from the client.
	channel: Option<mpsc::Receiver<RpcMessage>>,
	/// Requests that haven't received a response yet.
	pending_requests: HashMap<Id, oneshot::Sender<Result<Value, RpcError>>>,
	/// Subscription requests that haven't received a subscription id yet.
	pending_subscriptions: HashMap<Id, String>,
	/// A map from the subscription name to the subscription.
	subscriptions: HashMap<String, Subscription>,
	/// Incoming messages from the underlying transport.
	stream: TStream,
	/// Unprocessed incoming messages.
	incoming: VecDeque<(Id, Result<Value, RpcError>, Option<String>)>,
	/// Unprocessed outgoing messages.
	outgoing: VecDeque<String>,
	/// Outgoing messages from the underlying transport.
	sink: TSink,
}

impl<TSink, TStream> Duplex<TSink, TStream> {
	/// Creates a new `Duplex`.
	fn new(sink: TSink, stream: TStream, channel: mpsc::Receiver<RpcMessage>) -> Self {
		log::debug!("open");
		Duplex {
			request_builder: RequestBuilder::new(),
			channel: Some(channel),
			pending_requests: Default::default(),
			pending_subscriptions: Default::default(),
			subscriptions: Default::default(),
			stream,
			incoming: Default::default(),
			outgoing: Default::default(),
			sink,
		}
	}
}

/// Creates a new `Duplex`, along with a channel to communicate
pub fn duplex<TSink, TStream>(sink: TSink, stream: TStream) -> (Duplex<TSink, TStream>, RpcChannel) {
	let (sender, receiver) = mpsc::channel(0);
	let client = Duplex::new(sink, stream, receiver);
	(client, sender.into())
}

impl<TSink, TStream> Future for Duplex<TSink, TStream>
where
	TSink: Sink<SinkItem = String, SinkError = RpcError>,
	TStream: Stream<Item = String, Error = RpcError>,
{
	type Item = ();
	type Error = RpcError;

	fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
		// Handle requests from the client.
		log::debug!("handle requests from client");
		loop {
			// Check that the client channel is open
			let channel = match self.channel.as_mut() {
				Some(channel) => channel,
				None => break,
			};
			let msg = match channel.poll() {
				Ok(Async::Ready(Some(msg))) => msg,
				Ok(Async::Ready(None)) => {
					// When the channel is dropped we still need to finish
					// outstanding requests.
					self.channel.take();
					break;
				}
				Ok(Async::NotReady) => break,
				Err(()) => continue,
			};
			let request_str = match msg {
				RpcMessage::Call(msg) => {
					let (id, request_str) = self.request_builder.call_request(&msg);
					if self.pending_requests.contains_key(&id) {
						log::error!("reuse of request id {:?}", id);
					}
					self.pending_requests.insert(id, msg.sender);
					request_str
				}
				RpcMessage::Subscribe(msg) => {
					let (id, request_str) = self.request_builder.subscribe_request(
						msg.subscription.subscribe.clone(),
						msg.subscription.subscribe_params.clone(),
					);
					let subscription = Subscription::new(msg.sender, msg.subscription.unsubscribe);
					if self.subscriptions.contains_key(&msg.subscription.notification) {
						// TODO: handle better
						log::warn!(
							"overwritting existing subscription for {}",
							msg.subscription.notification
						);
					}
					self.subscriptions
						.insert(msg.subscription.notification.clone(), subscription);
					log::debug!("subscribed to {}", msg.subscription.notification);
					self.pending_subscriptions.insert(id, msg.subscription.notification);
					request_str
				}
			};
			log::debug!("outgoing: {}", request_str);
			self.outgoing.push_back(request_str);
		}

		// Handle stream.
		// Reads from stream and queues to incoming queue.
		log::debug!("handle stream");
		loop {
			let response_str = match self.stream.poll() {
				Ok(Async::Ready(Some(response_str))) => response_str,
				Ok(Async::Ready(None)) => {
					// The websocket connection was closed so the client
					// can be shutdown. Reopening closed connections must
					// be handled by the transport.
					debug!("connection closed");
					return Ok(Async::Ready(()));
				}
				Ok(Async::NotReady) => break,
				Err(err) => Err(err)?,
			};
			log::debug!("incoming: {}", response_str);
			for response in super::parse_response(&response_str)? {
				self.incoming.push_back(response);
			}
		}

		// Handle incoming queue.
		log::debug!("handle incoming");
		loop {
			match self.incoming.pop_front() {
				Some((id, result, method)) => match method {
					// is a notification
					Some(method) => match self.subscriptions.get_mut(&method) {
						Some(subscription) => match subscription.channel.poll_ready() {
							Ok(Async::Ready(())) => {
								subscription.channel.try_send(result).expect("shouldn't fail; qed");
							}
							Ok(Async::NotReady) => {
								self.incoming.push_front((id, result, Some(method)));
								break;
							}
							Err(_) => {
								let subscription = self.subscriptions.remove(&method).expect("subscription exists");
								match subscription.id {
									Some(sid) => {
										let (_id, request_str) =
											self.request_builder.unsubscribe_request(subscription.unsubscribe, sid);
										log::debug!("outgoing: {}", request_str);
										self.outgoing.push_back(request_str);
										log::debug!("unsubscribed from {}", method);
									}
									None => {
										// TODO: handle better
										log::warn!(
											"subscription cancelled before receiving a response from the server"
										);
									}
								}
							}
						},
						None => {
							log::warn!("unknown subscription {}", method);
						}
					},
					// is a response
					None => {
						if let Some(tx) = self.pending_requests.remove(&id) {
							tx.send(result)
								.map_err(|_| RpcError::Other(format_err!("oneshot channel closed")))?;
							continue;
						}
						if let Some(notification) = self.pending_subscriptions.remove(&id) {
							match result.map(|id| SubscriptionId::parse_value(&id)) {
								Ok(Some(sid)) => {
									// Ignore subscription if we already unsubscribed
									if let Some(subscription) = self.subscriptions.get_mut(&notification) {
										subscription.id = Some(sid);
									}
								}
								Ok(None) => log::warn!("received invalid id"),
								Err(err) => log::warn!("received invalid notification {:?}", err),
							}
							continue;
						}
						log::warn!("unknown id {:?}", id);
					}
				},
				None => break,
			}
		}

		// Handle outgoing queue.
		// Writes queued messages to sink.
		log::debug!("handle outgoing");
		loop {
			match self.outgoing.pop_front() {
				Some(request) => match self.sink.start_send(request)? {
					AsyncSink::Ready => {}
					AsyncSink::NotReady(request) => {
						self.outgoing.push_front(request);
						break;
					}
				},
				None => break,
			}
		}
		log::debug!("handle sink");
		let sink_empty = match self.sink.poll_complete()? {
			Async::Ready(()) => true,
			Async::NotReady => false,
		};

		log::debug!("{:?}", self);
		// Return ready when the future is complete
		if self.channel.is_none()
			&& self.outgoing.is_empty()
			&& self.incoming.is_empty()
			&& self.pending_requests.is_empty()
			&& self.pending_subscriptions.is_empty()
			&& self.subscriptions.is_empty()
			&& sink_empty
		{
			log::debug!("close");
			Ok(Async::Ready(()))
		} else {
			Ok(Async::NotReady)
		}
	}
}

impl<TSink, TStream> std::fmt::Debug for Duplex<TSink, TStream> {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		writeln!(f, "channel is none: {}", self.channel.is_none())?;
		writeln!(f, "outgoing: {}", self.outgoing.len())?;
		writeln!(f, "incoming: {}", self.incoming.len())?;
		writeln!(f, "pending_requests: {}", self.pending_requests.len())?;
		writeln!(f, "pending_subscriptions: {}", self.pending_subscriptions.len())?;
		writeln!(f, "subscriptions: {}", self.subscriptions.len())
	}
}
