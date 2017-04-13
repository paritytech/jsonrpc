use std::marker::PhantomData;

use jsonrpc_core as core;
use jsonrpc_pubsub as pubsub;
use serde;
use util::to_value;

use self::core::futures::{self, Sink as FuturesSink};

pub use self::pubsub::SubscriptionId;

pub struct Subscriber<T> {
	subscriber: pubsub::Subscriber,
	_data: PhantomData<T>,
}

impl<T> Subscriber<T> {
	pub fn new(subscriber: pubsub::Subscriber) -> Self {
		Subscriber {
			subscriber: subscriber,
			_data: PhantomData,
		}
	}

	pub fn reject(self, error: core::Error) -> Result<(), ()> {
		self.subscriber.reject(error)
	}

	pub fn assign_id(self, id: SubscriptionId) -> Result<Sink<T>, ()> {
		let sink = self.subscriber.assign_id(id.clone())?;
		Ok(Sink {
			id: id,
			sink: sink,
			buffered: None,
			_data: PhantomData,
		})
	}
}

pub struct Sink<T> {
	sink: pubsub::Sink,
	id: SubscriptionId,
	buffered: Option<core::Params>,
	_data: PhantomData<T>,
}

impl<T: serde::Serialize> Sink<T> {
	pub fn notify(&self, val: T) -> pubsub::SinkResult {
		self.sink.notify(self.val_to_params(val))
	}

	fn val_to_params(&self, val: T) -> core::Params {
		let id = self.id.clone().into();
		let val = to_value(val);

		core::Params::Map(vec![
			("subscription".to_owned(), id),
			("result".to_owned(), val),
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

impl<T: serde::Serialize> futures::sink::Sink for Sink<T> {
	type SinkItem = T;
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
