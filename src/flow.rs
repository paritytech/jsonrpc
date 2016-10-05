//! Response processing functions.

use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::Mutex;
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

impl<A: 'static> Handler<A, A> {
	pub fn id<X>(handler: X) -> Self where
		X: ResponseHandler<A> + 'static {
			Handler {
				handler: Box::new(handler),
				mapper: Box::new(|x| x),
			}
	}
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
	pub fn split_map<C, G>(self, count: usize, map: G) -> Vec<Handler<C, C>> where
		G: Fn(Vec<C>) -> B + Send + 'static,
		C: Send + 'static {
		let outputs = Arc::new(Mutex::new(Some(Vec::with_capacity(count))));
		let handler = Arc::new(Mutex::new(Some((self, map))));

		(0..count).into_iter().map(|i| {
			let outputs = outputs.clone();
			let handler = handler.clone();

			// / TODO [ToDr] What will happen in case of subscriptions?
			Handler::id(move |res| {
				let mut outputs = outputs.lock();
				let len = {
					let mut out = outputs.as_mut().expect("When output is taken no handlers are left.");
					// NOTE Order of responses does not really matter
					out.push(res);
					out.len()
				};

				// last handler
				if len == count {
					let (handler, map) = handler.lock().take().expect("Only single handler will be the last.");
					let outputs = outputs.take().expect("Outputs taken only once.");

					handler.send(map(outputs));
				}
			})
		}).collect()
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

#[cfg(test)]
mod tests {
	use std::sync::mpsc;
	use super::{Handler, ResponseHandler};

	#[test]
	fn should_map_handler_correctly() {
		// given
		let (tx, rx) = mpsc::channel();
		let handler = Handler::new(move |output| {
			tx.send(output).unwrap();
		}, |data: usize| data + 10);

		// when
		handler.send(20);

		// then
		assert_eq!(rx.recv().unwrap(), 30);
	}

	#[test]
	fn should_return_new_mapping_handler() {
		// given
		let (tx, rx) = mpsc::channel();
		let handler = Handler::new(move |output| {
			tx.send(output).unwrap();
		}, |data: usize| data + 10).map(|x| x + 15usize);

		// when
		handler.send(20);

		// then
		assert_eq!(rx.recv().unwrap(), 45);
	}

	#[test]
	fn should_split_handler() {
		// given
		let (tx, rx) = mpsc::channel();
		let handler = Handler::new(move |output| {
			tx.send(output).unwrap();
		}, |data: i64| data);
		// split handler
		let split = handler.split_map(2, |data: Vec<usize>| data.into_iter().fold(0, |a, b| a + b) as i64);
		assert_eq!(split.len(), 2);

		// when
		let mut split = split.into_iter();
		let a = split.next().unwrap();
		let b = split.next().unwrap();

		// then
		a.send(10);
		b.send(20);

		assert_eq!(rx.recv().unwrap(), 30i64);
	}


}
