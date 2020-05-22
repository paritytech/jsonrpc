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
use crate::core::futures::{future, Future};
use crate::{
	typed::{Sink, Subscriber},
	SubscriptionId,
};

use log::{error, warn};
use parking_lot::Mutex;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

/// Alias for an implementation of `futures::future::Executor`.
pub type TaskExecutor = Arc<dyn future::Executor<Box<dyn Future<Item = (), Error = ()> + Send>> + Send + Sync>;

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
#[derive(Debug)]
pub struct NumericIdProvider {
	current_id: AtomicUsize,
}

impl NumericIdProvider {
	/// Create a new NumericIdProvider.
	pub fn new() -> Self {
		Default::default()
	}

	/// Create a new NumericIdProvider starting from
	/// the given ID.
	pub fn with_id(id: AtomicUsize) -> Self {
		Self { current_id: id }
	}
}

impl IdProvider for NumericIdProvider {
	type Id = usize;

	fn next_id(&self) -> Self::Id {
		self.current_id.fetch_add(1, Ordering::AcqRel)
	}
}

impl Default for NumericIdProvider {
	fn default() -> Self {
		NumericIdProvider {
			current_id: AtomicUsize::new(1),
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

impl<I: IdProvider> SubscriptionManager<I> {
	/// Creates a new SubscriptionManager.
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
		R: future::IntoFuture<Future = F, Item = (), Error = ()>,
		F: future::Future<Item = (), Error = ()> + Send + 'static,
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
	pub fn new(executor: TaskExecutor) -> Self {
		Self {
			id_provider: Default::default(),
			active_subscriptions: Default::default(),
			executor,
		}
	}
}
