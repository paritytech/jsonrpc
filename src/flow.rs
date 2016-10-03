//! Response processing functions.

use std::sync::Arc;
use commander::SubscriptionCommand;
use super::{Value, Error, Params};

/// Convenient type for RPC methods return types.
pub type Data = Result<Value, Error>;

/// Subscription control object
pub enum Subscription {
	/// Open new subscription with given parameters
	Open {
		/// Parameters
		params: Params,
		/// Awaiting subscriber
		subscriber: NewSubscriber,
	},
	/// Close previously opened subscription
	Close {
		/// Id of subscription
		id: Value,
		/// Asynchronous response
		ready: Ready,
	},
}

/// Asynchronous method response.
pub struct Ready {
	handler: Box<ResponseHandler<Data>>,
}

impl Ready {
	/// Return a response to the caller
	pub fn ready(self, data: Data) {
		self.handler.send(data)
	}
}

/// New subscriber waiting for id to assign
pub struct NewSubscriber {
	session: Session,
	name: String,
	unsubscribe: Arc<Box<SubscriptionCommand>>,
	handler: Box<ResponseHandler<Data>>,
}

impl NewSubscriber {
	/// Rejects this subscriber
	pub fn reject(self, error: Error) {
		self.handler.send(Err(error));
	}

	/// Assign and send new ID to this subscriber.
	/// Converts this object into `Subscriber` which allows to send notifications to the client.
	pub fn assign_id(self, id: Value) -> Subscriber {
		// Send a response
		self.handler.send(Ok(id.clone()));
		// Add subscription
		self.session.add_subscription(self.name, id, self.unsubscribe);
		Subscriber {
			handler: self.handler,
		}
	}
}

/// Subscriber with assigned ID
pub struct Subscriber {
	handler: Box<ResponseHandler<Data>>,
}

impl Subscriber {
	/// Send a notification to subscriber.
	pub fn send(&self, data: Data) {
		self.handler.send(data);
	}
}

impl<A: 'static> From<Handler<A, Data>> for Ready {
	fn from(handler: Handler<A, Data>) -> Self {
		Ready {
			handler: Box::new(handler),
		}
	}
}

/// Object representing current session.
/// It's up to JSON-RPC transport to decide when sessions are created.
///
/// Subscriptions are only possible when session is provided.
/// Dropping session object will automatically unsubscribe from all previously opened subscriptions.
/// TODO [ToDr] Will leak when Session is dropped before subscription ID is assigned.
#[derive(Debug, Clone)]
pub struct Session {
}

impl Session {
	/// Adds new subscription to this session for auto-unsubscribe.
	fn add_subscription(&self, name: String, id: Value, unsubscribe: Arc<Box<SubscriptionCommand>>) {
		unimplemented!()
	}

	/// Removes a subscription from auto-unsubscribe. Client has manually unsubscribed.
	pub fn remove_subscription(&self, name: String, id: Value) {
		unimplemented!()
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		// TODO Clear all subscriptions
		unimplemented!()
	}
}

/// Trait representing a client waiting for the response(s).
pub trait ResponseHandler<T>: Send {
	/// Send a reponse to that client.
	fn send(&self, response: T);
}

impl<T, F: Fn(T) + Send> ResponseHandler<T> for F {
	fn send(&self, response: T) {
		self(response)
	}
}

/// Response handler with transformations.
pub struct Handler<A, B> {
	handler: Box<ResponseHandler<A>>,
	mapper: Box<Fn(B) -> A + Send>,
}

impl<A: 'static, B: 'static> Handler<A, B> {
	/// Create a new `Handler` with given transformation function.
	pub fn new<X, Y>(handler: X, mapper: Y) -> Self where
		X: ResponseHandler<A> + 'static,
		Y: Fn(B) -> A + Send + 'static {
			Handler {
				handler: Box::new(handler),
				mapper: Box::new(mapper),
			}
	}

	/// Convert this `Handler` into a new one, accepting different input.
	pub fn map<C, G>(self, map: G) -> Handler<A, C> where
		G: Fn(C) -> B + Send + 'static {
		let current = self.mapper;
		Handler {
			handler: self.handler,
			mapper: Box::new(move |c| current(map(c))),
		}
	}

	/// Split this handler into `count` handlers.
	/// Upstream `ResponseHandler` will be called only when all sub-handlers receive a response.
	pub fn split_map<C, G>(self, count: usize, map: G) -> Vec<Handler<A, C>> where
		G: Fn(Vec<C>) -> B + 'static {
		unimplemented!()
	}
}

impl<A: 'static> Handler<A, Data> {
	/// Converts this handler into a `NewSubscriber` for given `Session`.
	pub fn into_subscriber(self, session: Session, name: String, unsubscribe: Arc<Box<SubscriptionCommand>>) -> NewSubscriber {
		NewSubscriber {
			session: session,
			name: name,
			unsubscribe: unsubscribe,
			handler: Box::new(self),
		}
	}
}

impl<A, B> ResponseHandler<B> for Handler<A, B> {
	fn send(&self, response: B) {
		let map = &self.mapper;
		self.handler.send(map(response))
	}
}
