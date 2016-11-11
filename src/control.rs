//! Response processing functions.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::Mutex;

use commander::SubscriptionCommand;
use error::Error;
use params::Params;
use super::Value;

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

impl fmt::Debug for Subscription {
	fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		let mut builder = f.debug_struct("Subscription");

		match *self {
			Subscription::Open { ref params, .. } => {
				builder.field("variant", &"open").field("params", params)
			},
			Subscription::Close { ref id, .. } => {
				builder.field("variant", &"close").field("id", id)
			},
		}.finish()
	}
}

impl PartialEq for Subscription {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(&Subscription::Open { params: ref p1, .. }, &Subscription::Open { params: ref p2, ..}) => {
				p1.eq(p2)
			},
			(&Subscription::Close { id: ref id1, ..}, &Subscription::Close { id: ref id2, ..}) => {
				id1.eq(id2)
			},
			_ => false,
		}
	}
}

/// Asynchronous method response.
pub struct Ready {
	handler: Box<ResponseHandler<Data>>,
}

impl Ready {
	fn discard() -> Self {
		Ready {
			handler: Box::new(|_| {}),
		}
	}

	/// Return a response to the caller
	pub fn ready(self, data: Data) {
		self.handler.send(data)
	}
}

fn _assert_ready_send() {
    fn _assert_send<T: Send>() {}
    _assert_send::<Ready>();
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
#[derive(Clone, Default)]
pub struct Session {
	session: Arc<Mutex<SessionInternal>>,
}

impl Session {
	/// Adds new subscription to this session for auto-unsubscribe.
	fn add_subscription(&self, name: String, id: Value, unsubscribe: Arc<Box<SubscriptionCommand>>) {
		self.session.lock().add_subscription(name, id, unsubscribe);
	}

	/// Removes a subscription from auto-unsubscribe. Client has manually unsubscribed.
	pub fn remove_subscription(&self, name: String, id: Value) {
		self.session.lock().remove_subscription(name, id);
	}
}

#[derive(Default)]
struct SessionInternal {
	subscriptions: Vec<(String, Value, Arc<Box<SubscriptionCommand>>)>,
}

impl SessionInternal {
	fn add_subscription(&mut self, name: String, id: Value, unsubscribe: Arc<Box<SubscriptionCommand>>) {
		self.subscriptions.push((name, id, unsubscribe));
	}

	fn remove_subscription(&mut self, name: String, id: Value) {
		let new = self.subscriptions.drain(..).filter(|&(ref name1, ref id1, _)| name1 != &name && id1 != &id).collect();
		self.subscriptions = new;
	}
}

impl Drop for SessionInternal {
	fn drop(&mut self) {
		for (_name, id, unsubscribe) in self.subscriptions.drain(..) {
			unsubscribe.execute(Subscription::Close {
				id: id,
				ready: Ready::discard(),
			});
		}
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
	/// Creates new identity handler.
	/// There is no mapping, same type is send upstream.
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
	/// Upstream `ResponseHandler` will be called only when all sub-handlers receive a response for the first time.
	///
	/// Some handlers may return more then one response - in such case:
	/// 1. First response of each handler is included in initial upstream response (`map_many`).
	/// 2. All subsequent responses are mapped using `map_single`.
	/// 3. Responses arriving after FIRST but before sending INITIAL upstream response are discarded.
	pub fn split_map<C, G, H>(self, count: usize, map_many: G, map_single: H) -> Vec<Handler<C, C>> where
		G: Fn(Vec<C>) -> B + Send + 'static,
		H: Fn(C) -> B + Send + 'static,
		C: Send + 'static {

		// If batch is empty we can respond right away with empty vector of responses.
		if count == 0 {
			self.send(map_many(vec![]));
			return vec![];
		}
		// Otherwise we need to wait for all requests in batch to respond
		// before sending a response to batch request.
		//
		// Further messages from the same handlers are notifications:
		// 1. We need to forward notifications, but only if initial response was sent.
		// 2. Notifications coming before sending the initial response are discarded.

		// Collecting responses for batch response.
		let outputs = Arc::new(Mutex::new(Some(Vec::with_capacity(count))));
		// Shared handle and map functions
		let handler = Arc::new(Mutex::new((self, map_many, map_single)));
		// Is the initial response sent already?
		let initial = Arc::new(AtomicBool::new(false));

		// For each request in batch
		(0..count).into_iter().map(|_| {
			let outputs = outputs.clone();
			let handler = handler.clone();
			let initial_sent = initial.clone();

			// Is response for this request already included in initial response (batch)?
			let my_response_sent = AtomicBool::new(false);

			Handler::id(move |res| {
				// If initial response was sent we are safe to forward notifications
				if initial_sent.load(Ordering::SeqCst) {
					// Just forward the message
					let lock = handler.lock();
					let (ref handler, _, ref map_single) = *lock;
					handler.send(map_single(res));
					return;
				}

				// Dicard notifications if initial response was not sent yet.
				if my_response_sent.load(Ordering::SeqCst) {
					return;
				}

				// It's the first response for this request, we need to include it
				// into initial response.
				my_response_sent.store(true, Ordering::SeqCst);

				let mut outputs = outputs.lock();
				let len = {
					let mut out = outputs.as_mut().expect("When output is taken no handlers are left.");
					// NOTE Order of responses does not really matter
					out.push(res);
					out.len()
				};

				// last handler - actually send initial_response
				if len == count {
					let outputs = outputs.take().expect("Outputs taken only once.");
					let lock = handler.lock();
					let (ref handler, ref map, _) = *lock;
					handler.send(map(outputs));
					initial_sent.store(true, Ordering::SeqCst);
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
	use std::sync::{mpsc, Arc};
	use parking_lot::Mutex;

	use Value;
	use error::Error;
	use commander::SubscriptionCommand;
	use super::{Handler, ResponseHandler, Session, Subscription, Ready};

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
		let split = handler.split_map(
			2,
			|data: Vec<usize>| data.into_iter().fold(0, |a, b| a + b) as i64,
			|single| single as i64,
		);
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

	#[test]
	fn should_split_handler_and_send_more_events_afterwards() {
		// given
		let (tx, rx) = mpsc::channel();
		let handler = Handler::new(move |output| {
			tx.send(output).unwrap();
		}, |data: i64| data);
		// split handler
		let split = handler.split_map(
			2,
			|data: Vec<usize>| data.into_iter().fold(0, |a, b| a + b) as i64,
			|single| (single + 5) as i64,
		);
		assert_eq!(split.len(), 2);

		// when
		let mut split = split.into_iter();
		let a = split.next().unwrap();
		let b = split.next().unwrap();

		// then
		a.send(10);
		a.send(30); // This message should be discarded
		b.send(20);

		a.send(50); // This should be propagated
		b.send(100); // And this too

		assert_eq!(rx.recv().unwrap(), 30i64);
		assert_eq!(rx.recv().unwrap(), 55i64);
		assert_eq!(rx.recv().unwrap(), 105i64);
	}

	#[test]
	fn should_handle_empty_batch() {
		let (tx, rx) = mpsc::channel();
		let handler = Handler::new(move |output| {
			tx.send(output).unwrap();
		}, |data: i64| data);

		// when
		let split = handler.split_map(
			0,
			|data: Vec<usize>| data.into_iter().fold(0, |a, b| a + b) as i64,
			|single| (single + 5) as i64,
		);
		assert_eq!(split.len(), 0);

		// then
		assert_eq!(rx.recv().unwrap(), 0);
	}

	#[test]
	fn should_unsubscribe_when_session_is_dropped() {
		// given
		let (tx, rx) = mpsc::channel();
		let tx = Mutex::new(tx);
		let session = Session::default();
		let command = Arc::new(Box::new(move |x: Subscription| tx.lock().send(x).unwrap()) as Box<SubscriptionCommand>);

		// when
		session.add_subscription("a".into(), Value::String("1".into()), command);
		drop(session);

		// then
		assert_eq!(rx.recv().unwrap(), Subscription::Close { id: Value::String("1".into()), ready: Ready::discard() });
	}

	#[test]
	fn should_not_unsubscribe_if_removed_manually() {
		// given
		let (tx, rx) = mpsc::channel();
		let tx = Mutex::new(tx);
		let session = Session::default();
		let command = Arc::new(Box::new(move |x: Subscription| tx.lock().send(x).unwrap()) as Box<SubscriptionCommand>);

		// when
		session.add_subscription("a".into(), Value::String("1".into()), command);
		session.remove_subscription("a".into(), Value::String("1".into()));
		drop(session);

		// then
		assert!(rx.recv().is_err(), "Should not get anything!");
	}

	#[test]
	fn should_convert_handler_into_ready() {
		// given
		let (tx, rx) = mpsc::channel();
		let handler = Handler::id(move |output| {
			tx.send(output).unwrap();
		});

		// when
		let ready: Ready = handler.into();
		ready.ready(Ok(Value::String("1".into())));

		// then
		assert_eq!(rx.recv().unwrap(), Ok(Value::String("1".into())));
	}

	#[test]
	fn should_convert_handler_into_subsciber_and_accept() {
		// given
		let (tx, rx) = mpsc::channel();
		let session = Session::default();
		let command = Arc::new(Box::new(move |_: Subscription| {}) as Box<SubscriptionCommand>);
		let handler = Handler::id(move |output| {
			tx.send(output).unwrap();
		});

		// when
		let new_subscriber = handler.into_subscriber(session, "a".into(), command);
		let subscriber = new_subscriber.assign_id(Value::U64(1));
		subscriber.send(Ok(Value::String("hello".into())));

		// then
		assert_eq!(rx.recv().unwrap(), Ok(Value::U64(1)));
		assert_eq!(rx.recv().unwrap(), Ok(Value::String("hello".into())));
		drop(subscriber);
		assert!(rx.recv().is_err(), "Should not receive anything else.");
	}

	#[test]
	fn should_convert_handler_into_subsciber_and_reject() {
		// given
		let (tx, rx) = mpsc::channel();
		let session = Session::default();
		let command = Arc::new(Box::new(move |_: Subscription| {}) as Box<SubscriptionCommand>);
		let handler = Handler::id(move |output| {
			tx.send(output).unwrap();
		});

		// when
		let new_subscriber = handler.into_subscriber(session, "a".into(), command);
		new_subscriber.reject(Error::invalid_request());

		// then
		assert_eq!(rx.recv().unwrap(), Err(Error::invalid_request()));
		assert!(rx.recv().is_err(), "Should not receive anything else.");
	}
}
