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
	/// A method name used for notification.
	notification: String,
	/// Rpc method to unsubscribe.
	unsubscribe: String,
	/// Where to send messages to.
	channel: mpsc::Sender<Result<Value, RpcError>>,
}

impl Subscription {
	fn new(channel: mpsc::Sender<Result<Value, RpcError>>, notification: String, unsubscribe: String) -> Self {
		Subscription {
			id: None,
			notification,
			unsubscribe,
			channel,
		}
	}
}

enum PendingRequest {
	Call(oneshot::Sender<Result<Value, RpcError>>),
	Subscription(Subscription),
}

/// The Duplex handles sending and receiving asynchronous
/// messages through an underlying transport.
pub struct Duplex<TSink, TStream> {
	request_builder: RequestBuilder,
	/// Channel from the client.
	channel: Option<mpsc::Receiver<RpcMessage>>,
	/// Requests that haven't received a response yet.
	pending_requests: HashMap<Id, PendingRequest>,
	/// A map from the subscription name to the subscription.
	subscriptions: HashMap<(SubscriptionId, String), Subscription>,
	/// Incoming messages from the underlying transport.
	stream: TStream,
	/// Unprocessed incoming messages.
	incoming: VecDeque<(Id, Result<Value, RpcError>, Option<String>, Option<SubscriptionId>)>,
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

					if self
						.pending_requests
						.insert(id.clone(), PendingRequest::Call(msg.sender))
						.is_some()
					{
						log::error!("reuse of request id {:?}", id);
					}
					request_str
				}
				RpcMessage::Subscribe(msg) => {
					let crate::Subscription {
						subscribe,
						subscribe_params,
						notification,
						unsubscribe,
					} = msg.subscription;
					let (id, request_str) = self.request_builder.subscribe_request(subscribe, subscribe_params);
					log::debug!("subscribing to {}", notification);
					let subscription = Subscription::new(msg.sender, notification, unsubscribe);
					if self
						.pending_requests
						.insert(id.clone(), PendingRequest::Subscription(subscription))
						.is_some()
					{
						log::error!("reuse of request id {:?}", id);
					}
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
			// we only send one request at the time, so there can only be one response.
			let (id, result, method, sid) = super::parse_response(&response_str)?;
			log::debug!(
				"id: {:?} (sid: {:?}) result: {:?} method: {:?}",
				id,
				sid,
				result,
				method
			);
			self.incoming.push_back((id, result, method, sid));
		}

		// Handle incoming queue.
		log::debug!("handle incoming");
		loop {
			match self.incoming.pop_front() {
				Some((id, result, method, sid)) => {
					let sid_and_method = sid.and_then(|sid| method.map(|method| (sid, method)));
					// Handle the response to a pending request.
					match self.pending_requests.remove(&id) {
						// It's a regular Req-Res call, so just answer.
						Some(PendingRequest::Call(tx)) => {
							tx.send(result)
								.map_err(|_| RpcError::Other(format_err!("oneshot channel closed")))?;
							continue;
						}
						// It was a subscription request,
						// turn it into a proper subscription.
						Some(PendingRequest::Subscription(mut subscription)) => {
							let sid = result.as_ref().ok().and_then(|res| SubscriptionId::parse_value(res));
							let method = subscription.notification.clone();

							if let Some(sid) = sid {
								subscription.id = Some(sid.clone());
								if self
									.subscriptions
									.insert((sid.clone(), method.clone()), subscription)
									.is_some()
								{
									log::warn!(
										"Overwriting existing subscription under {:?} ({:?}). \
										 Seems that server returned the same subscription id.",
										sid,
										method,
									);
								}
							} else {
								log::warn!(
									"The response does not have expected subscription id in response: {:?} ({:?}): {:?}",
									id,
									method,
									result,
								);
							}
							continue;
						}
						// It's not a pending request nor a notification
						None if sid_and_method.is_none() => {
							log::warn!("Got unexpected response with id {:?} ({:?})", id, sid_and_method);
							continue;
						}
						// just fall-through in case it's a notification
						None => {}
					};

					let sid_and_method = if let Some(x) = sid_and_method {
						x
					} else {
						continue;
					};

					if let Some(subscription) = self.subscriptions.get_mut(&sid_and_method) {
						match subscription.channel.poll_ready() {
							Ok(Async::Ready(())) => {
								subscription
									.channel
									.try_send(result)
									.expect("The channel is ready; qed");
							}
							Ok(Async::NotReady) => {
								let (sid, method) = sid_and_method;
								self.incoming.push_back((id, result, Some(method), Some(sid)));
								break;
							}
							Err(_) => {
								let subscription = self
									.subscriptions
									.remove(&sid_and_method)
									.expect("Subscription was just polled; qed");
								let sid = subscription.id.expect(
									"Every subscription that ends up in `self.subscriptions` has id already \
									 assigned; assignment happens during response to subscribe request.",
								);
								let (_id, request_str) =
									self.request_builder.unsubscribe_request(subscription.unsubscribe, sid);
								log::debug!("outgoing: {}", request_str);
								self.outgoing.push_back(request_str);
								log::debug!("unsubscribed from {:?}", sid_and_method);
							}
						}
					} else {
						log::warn!("Received unexpected subscription notification: {:?}", sid_and_method);
					}
				}
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
		writeln!(f, "subscriptions: {}", self.subscriptions.len())?;
		Ok(())
	}
}
