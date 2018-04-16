//! PUB-SUB auto-serializing structures.

use std::marker::PhantomData;

use jsonrpc_core as core;
use jsonrpc_pubsub as pubsub;
use serde;
use util::to_value;

use self::core::futures::{self, Sink as FuturesSink, sync};

pub use self::pubsub::SubscriptionId;

/// New PUB-SUB subcriber.
#[derive(Debug)]
pub struct Subscriber<T, E = core::Error> {
	subscriber: pubsub::Subscriber,
	_data: PhantomData<(T, E)>,
}

impl<T, E> Subscriber<T, E> {
	/// Wrap non-typed subscriber.
	pub fn new(subscriber: pubsub::Subscriber) -> Self {
		Subscriber {
			subscriber: subscriber,
			_data: PhantomData,
		}
	}

	/// Create new subscriber for tests.
	pub fn new_test<M: Into<String>>(method: M) -> (
		Self,
		sync::oneshot::Receiver<Result<SubscriptionId, core::Error>>,
		sync::mpsc::Receiver<String>,
	) {
		let (subscriber, id, subscription) = pubsub::Subscriber::new_test(method);
		(Subscriber::new(subscriber), id, subscription)
	}

	/// Reject subscription with given error.
	pub fn reject(self, error: core::Error) -> Result<(), ()> {
		self.subscriber.reject(error)
	}

	/// Assign id to this subscriber.
	/// This method consumes `Subscriber` and returns `Sink`
	/// if the connection is still open or error otherwise.
	pub fn assign_id(self, id: SubscriptionId) -> Result<Sink<T, E>, ()> {
		let sink = self.subscriber.assign_id(id.clone())?;
		Ok(Sink {
			id: id,
			sink: sink,
			buffered: None,
			_data: PhantomData,
		})
	}
}

/// Subscriber sink.
#[derive(Debug, Clone)]
pub struct Sink<T, E = core::Error> {
	sink: pubsub::Sink,
	id: SubscriptionId,
	buffered: Option<core::Params>,
	_data: PhantomData<(T, E)>,
}

impl<T: serde::Serialize, E: serde::Serialize> Sink<T, E> {
	/// Sends a notification to the subscriber.
	pub fn notify(&self, val: Result<T, E>) -> pubsub::SinkResult {
		self.sink.notify(self.val_to_params(val))
	}

	fn val_to_params(&self, val: Result<T, E>) -> core::Params {
		let id = self.id.clone().into();
		let val = val.map(to_value).map_err(to_value);

		core::Params::Map(vec![
			("subscription".to_owned(), id),
			match val {
				Ok(val) => ("result".to_owned(), val),
				Err(err) => ("error".to_owned(), err),
			},
		].into_iter().collect())
	}

	fn poll(&mut self) -> futures::Poll<(), pubsub::TransportError> {
		if let Some(item) = self.buffered.take() {
			let result = self.sink.start_send(item)?;
			if let futures::AsyncSink::NotReady(item) = result {
				self.buffered = Some(item);
			}
		}

		if self.buffered.is_some() {
			Ok(futures::Async::NotReady)
		} else {
			Ok(futures::Async::Ready(()))
		}
	}
}

impl<T: serde::Serialize, E: serde::Serialize> futures::sink::Sink for Sink<T, E> {
	type SinkItem = Result<T, E>;
	type SinkError = pubsub::TransportError;

	fn start_send(&mut self, item: Self::SinkItem) -> futures::StartSend<Self::SinkItem, Self::SinkError> {
		// Make sure to always try to process the buffered entry.
		// Since we're just a proxy to real `Sink` we don't need
		// to schedule a `Task` wakeup. It will be done downstream.
		if self.poll()?.is_not_ready() {
			return Ok(futures::AsyncSink::NotReady(item));
		}

		let val = self.val_to_params(item);
		self.buffered = Some(val);
		self.poll()?;

		Ok(futures::AsyncSink::Ready)
	}

	fn poll_complete(&mut self) -> futures::Poll<(), Self::SinkError> {
		self.poll()?;
		self.sink.poll_complete()
	}

	fn close(&mut self) -> futures::Poll<(), Self::SinkError> {
		self.poll()?;
		self.sink.close()
	}
}
