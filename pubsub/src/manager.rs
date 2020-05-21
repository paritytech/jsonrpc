//! Provides an executor for subscription Futures.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use crate::core::futures::sync::oneshot;
use crate::core::futures::{future, Future};
use crate::{
	typed::{Sink, Subscriber},
	SubscriptionId,
};
use log::{error, warn};
use parking_lot::Mutex;

/// Alias for an implementation of `futures::future::Executor`.
pub type TaskExecutor = Arc<dyn future::Executor<Box<dyn Future<Item = (), Error = ()> + Send>> + Send + Sync>;

/// Trait used to provide unique subscription ids.
pub trait IdProvider {
	/// A unique ID used to identify a subscription.
	type Id: Copy + Clone + Default + Eq + Hash + Into<SubscriptionId> + From<SubscriptionId>;

	/// Returns next id for the subscription.
	fn next_id(&self) -> Self::Id;
}

/// Subscriptions manager.
///
/// Takes care of assigning unique subscription ids and
/// driving the sinks into completion.
#[derive(Clone)]
pub struct SubscriptionManager<I: Default + IdProvider> {
	next_id: I,
	active_subscriptions: Arc<Mutex<HashMap<I::Id, oneshot::Sender<()>>>>,
	executor: TaskExecutor, // Make generic?
}

impl<I: Default + IdProvider> SubscriptionManager<I> {
	/// Creates a new SubscriptionManager.
	pub fn new(executor: TaskExecutor) -> Self {
		Self {
			next_id: Default::default(),
			active_subscriptions: Default::default(),
			executor,
		}
	}

	fn executor(&self) -> &TaskExecutor {
		&self.executor
	}

	fn add<T, E, G, R, F>(&self, subscriber: Subscriber<T, E>, into_future: G) -> SubscriptionId
	where
		G: FnOnce(Sink<T, E>) -> R,
		R: future::IntoFuture<Future = F, Item = (), Error = ()>,
		F: future::Future<Item = (), Error = ()> + Send + 'static,
	{
		let id = self.next_id.next_id();
		let subscription_id: SubscriptionId = id.into();
		if let Ok(sink) = subscriber.assign_id(subscription_id.clone()) {
			let (tx, rx) = oneshot::channel();
			let future = into_future(sink)
				.into_future()
				.select(rx.map_err(|e| warn!("Error timing out: {:?}", e)))
				.then(|_| Ok(()));

			self.active_subscriptions.lock().insert(id, tx);
			if self.executor.execute(Box::new(future)).is_err() {
				error!("Failed to spawn RPC subscription task");
			}
		}

		subscription_id
	}

	/// Cancel subscription.
	///
	/// Returns true if subscription existed or false otherwise.
	fn cancel(&self, id: SubscriptionId) -> bool {
		if let Some(tx) = self.active_subscriptions.lock().remove(&id.into()) {
			let _ = tx.send(());
			return true;
		}

		false
	}
}
