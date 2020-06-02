//! The SubscriptionManager used to manage subscription based RPCs.
//!
//! The manager provides four main things in terms of functionality:
//!
//! 1. The ability to create unique subscription IDs through the
//! use of the `IdProvider` trait. Two implementations are availble
//! out of the box, a `NumericIdProvider` and a `RandomStringIdProvider`.
//!
//! 2. An executor with which to drive `Future`s to completion.
//!
//! 3. A way to add new subscriptions. Subscriptions should come in the form
//! of a `Stream`. These subscriptions will be transformed into notifications
//! by the manager, which can be consumed by the client.
//!
//! 4. A way to cancel any currently active subscription.

use std::collections::HashMap;
use std::iter;
use std::sync::{
	atomic::{AtomicUsize, Ordering},
	Arc,
};

use crate::core::futures::sync::oneshot;
use crate::core::futures::{future as future01, Future as Future01};
use crate::{
	typed::{Sink, Subscriber},
	SubscriptionId,
};

use log::{error, warn};
use parking_lot::Mutex;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

/// Alias for an implementation of `futures::future::Executor`.
pub type TaskExecutor = Arc<dyn future01::Executor<Box<dyn Future01<Item = (), Error = ()> + Send>> + Send + Sync>;

type ActiveSubscriptions = Arc<Mutex<HashMap<SubscriptionId, oneshot::Sender<()>>>>;

/// Trait used to provide unique subscription IDs.
pub trait IdProvider {
	/// A unique ID used to identify a subscription.
	type Id: Default + Into<SubscriptionId>;

	/// Returns the next ID for the subscription.
	fn next_id(&self) -> Self::Id;
}

/// Provides a thread-safe incrementing integer which
/// can be used as a subscription ID.
#[derive(Clone, Debug)]
pub struct NumericIdProvider {
	current_id: Arc<AtomicUsize>,
}

impl NumericIdProvider {
	/// Create a new NumericIdProvider.
	pub fn new() -> Self {
		Default::default()
	}

	/// Create a new NumericIdProvider starting from
	/// the given ID.
	pub fn with_id(id: AtomicUsize) -> Self {
		Self {
			current_id: Arc::new(id),
		}
	}
}

impl IdProvider for NumericIdProvider {
	type Id = u64;

	fn next_id(&self) -> Self::Id {
		self.current_id.fetch_add(1, Ordering::AcqRel) as u64
	}
}

impl Default for NumericIdProvider {
	fn default() -> Self {
		NumericIdProvider {
			current_id: Arc::new(AtomicUsize::new(1)),
		}
	}
}

/// Used to generate random strings for use as
/// subscription IDs.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct RandomStringIdProvider {
	len: usize,
}

impl RandomStringIdProvider {
	/// Create a new RandomStringIdProvider.
	pub fn new() -> Self {
		Default::default()
	}

	/// Create a new RandomStringIdProvider, which will generate
	/// random id strings of the given length.
	pub fn with_len(len: usize) -> Self {
		Self { len }
	}
}

impl IdProvider for RandomStringIdProvider {
	type Id = String;

	fn next_id(&self) -> Self::Id {
		let mut rng = thread_rng();
		let id: String = iter::repeat(())
			.map(|()| rng.sample(Alphanumeric))
			.take(self.len)
			.collect();
		id
	}
}

impl Default for RandomStringIdProvider {
	fn default() -> Self {
		Self { len: 16 }
	}
}

/// Subscriptions manager.
///
/// Takes care of assigning unique subscription ids and
/// driving the sinks into completion.
#[derive(Clone)]
pub struct SubscriptionManager<I: IdProvider = RandomStringIdProvider> {
	id_provider: I,
	active_subscriptions: ActiveSubscriptions,
	executor: TaskExecutor,
}

impl SubscriptionManager {
	/// Creates a new SubscriptionManager.
	///
	/// Uses `RandomStringIdProvider` as the ID provider.
	pub fn new(executor: TaskExecutor) -> Self {
		Self {
			id_provider: RandomStringIdProvider::default(),
			active_subscriptions: Default::default(),
			executor,
		}
	}
}

impl<I: IdProvider> SubscriptionManager<I> {
	/// Creates a new SubscriptionManager with the specified
	/// ID provider.
	pub fn with_id_provider(id_provider: I, executor: TaskExecutor) -> Self {
		Self {
			id_provider,
			active_subscriptions: Default::default(),
			executor,
		}
	}

	/// Borrows the internal task executor.
	///
	/// This can be used to spawn additional tasks on the underlying event loop.
	pub fn executor(&self) -> &TaskExecutor {
		&self.executor
	}

	/// Creates new subscription for given subscriber.
	///
	/// Second parameter is a function that converts Subscriber Sink into a Future.
	/// This future will be driven to completion by the underlying event loop
	pub fn add<T, E, G, R, F>(&self, subscriber: Subscriber<T, E>, into_future: G) -> SubscriptionId
	where
		G: FnOnce(Sink<T, E>) -> R,
		R: future01::IntoFuture<Future = F, Item = (), Error = ()>,
		F: future01::Future<Item = (), Error = ()> + Send + 'static,
	{
		let id = self.id_provider.next_id();
		let subscription_id: SubscriptionId = id.into();
		if let Ok(sink) = subscriber.assign_id(subscription_id.clone()) {
			let (tx, rx) = oneshot::channel();
			let future = into_future(sink)
				.into_future()
				.select(rx.map_err(|e| warn!("Error timing out: {:?}", e)))
				.then(|_| Ok(()));

			self.active_subscriptions.lock().insert(subscription_id.clone(), tx);
			if self.executor.execute(Box::new(future)).is_err() {
				error!("Failed to spawn RPC subscription task");
			}
		}

		subscription_id
	}

	/// Cancel subscription.
	///
	/// Returns true if subscription existed or false otherwise.
	pub fn cancel(&self, id: SubscriptionId) -> bool {
		if let Some(tx) = self.active_subscriptions.lock().remove(&id) {
			let _ = tx.send(());
			return true;
		}

		false
	}
}

impl<I: Default + IdProvider> SubscriptionManager<I> {
	/// Creates a new SubscriptionManager.
	pub fn with_executor(executor: TaskExecutor) -> Self {
		Self {
			id_provider: Default::default(),
			active_subscriptions: Default::default(),
			executor,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::typed::Subscriber;
	use futures::{compat::Future01CompatExt, executor, FutureExt};
	use futures::{stream, StreamExt, TryStreamExt};

	use crate::core::futures::sink::Sink as Sink01;
	use crate::core::futures::stream::Stream as Stream01;

	// Executor shared by all tests.
	//
	// This shared executor is used to prevent `Too many open files` errors
	// on systems with a lot of cores.
	lazy_static::lazy_static! {
		static ref EXECUTOR: executor::ThreadPool = executor::ThreadPool::new()
			.expect("Failed to create thread pool executor for tests");
	}

	pub struct TestTaskExecutor;
	type Boxed01Future01 = Box<dyn future01::Future<Item = (), Error = ()> + Send + 'static>;

	impl future01::Executor<Boxed01Future01> for TestTaskExecutor {
		fn execute(&self, future: Boxed01Future01) -> std::result::Result<(), future01::ExecuteError<Boxed01Future01>> {
			EXECUTOR.spawn_ok(future.compat().map(drop));
			Ok(())
		}
	}

	#[test]
	fn making_a_numeric_id_provider_works() {
		let provider = NumericIdProvider::new();
		let expected_id = 1;
		let actual_id = provider.next_id();

		assert_eq!(actual_id, expected_id);
	}

	#[test]
	fn default_numeric_id_provider_works() {
		let provider: NumericIdProvider = Default::default();
		let expected_id = 1;
		let actual_id = provider.next_id();

		assert_eq!(actual_id, expected_id);
	}

	#[test]
	fn numeric_id_provider_with_id_works() {
		let provider = NumericIdProvider::with_id(AtomicUsize::new(5));
		let expected_id = 5;
		let actual_id = provider.next_id();

		assert_eq!(actual_id, expected_id);
	}

	#[test]
	fn random_string_provider_returns_id_with_correct_default_len() {
		let provider = RandomStringIdProvider::new();
		let expected_len = 16;
		let actual_len = provider.next_id().len();

		assert_eq!(actual_len, expected_len);
	}

	#[test]
	fn random_string_provider_returns_id_with_correct_user_given_len() {
		let expected_len = 10;
		let provider = RandomStringIdProvider::with_len(expected_len);
		let actual_len = provider.next_id().len();

		assert_eq!(actual_len, expected_len);
	}

	#[test]
	fn new_subscription_manager_defaults_to_random_string_provider() {
		let manager = SubscriptionManager::new(Arc::new(TestTaskExecutor));
		let subscriber = Subscriber::<u64>::new_test("test_subTest").0;
		let stream = stream::iter(vec![Ok(1)]).compat();

		let id = manager.add(subscriber, |sink| {
			let stream = stream.map(|res| Ok(res));

			sink.sink_map_err(|_| ()).send_all(stream).map(|_| ())
		});

		assert!(matches!(id, SubscriptionId::String(_)))
	}

	#[test]
	fn new_subscription_manager_works_with_numeric_id_provider() {
		let id_provider = NumericIdProvider::default();
		let manager = SubscriptionManager::with_id_provider(id_provider, Arc::new(TestTaskExecutor));

		let subscriber = Subscriber::<u64>::new_test("test_subTest").0;
		let stream = stream::iter(vec![Ok(1)]).compat();

		let id = manager.add(subscriber, |sink| {
			let stream = stream.map(|res| Ok(res));

			sink.sink_map_err(|_| ()).send_all(stream).map(|_| ())
		});

		assert!(matches!(id, SubscriptionId::Number(_)))
	}

	#[test]
	fn new_subscription_manager_works_with_random_string_provider() {
		let id_provider = RandomStringIdProvider::default();
		let manager = SubscriptionManager::with_id_provider(id_provider, Arc::new(TestTaskExecutor));

		let subscriber = Subscriber::<u64>::new_test("test_subTest").0;
		let stream = stream::iter(vec![Ok(1)]).compat();

		let id = manager.add(subscriber, |sink| {
			let stream = stream.map(|res| Ok(res));

			sink.sink_map_err(|_| ()).send_all(stream).map(|_| ())
		});

		assert!(matches!(id, SubscriptionId::String(_)))
	}

	#[test]
	fn subscription_is_canceled_if_it_existed() {
		let manager = SubscriptionManager::<NumericIdProvider>::with_executor(Arc::new(TestTaskExecutor));
		// Need to bind receiver here (unlike the other tests) or else the subscriber
		// will think the client has disconnected and not update `active_subscriptions`
		let (subscriber, _recv, _) = Subscriber::<u64>::new_test("test_subTest");

		let (mut tx, rx) = futures::channel::mpsc::channel(8);
		tx.start_send(1).unwrap();
		let stream = rx.map(|v| Ok::<_, ()>(v)).compat();

		let id = manager.add(subscriber, |sink| {
			let stream = stream.map(|res| Ok(res));

			sink.sink_map_err(|_| ()).send_all(stream).map(|_| ())
		});

		let is_cancelled = manager.cancel(id);
		assert!(is_cancelled);
	}

	#[test]
	fn subscription_is_not_canceled_because_it_didnt_exist() {
		let manager = SubscriptionManager::new(Arc::new(TestTaskExecutor));

		let id: SubscriptionId = 23u32.into();
		let is_cancelled = manager.cancel(id);
		let is_not_cancelled = !is_cancelled;

		assert!(is_not_cancelled);
	}
}
