use std::marker::PhantomData;

use jsonrpc_core as core;
use jsonrpc_pubsub as pubsub;
use serde;
use util::to_value;

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

	pub fn reject(self, error: core::Error) {
		self.subscriber.reject(error)
	}

	pub fn assign_id(self, id: SubscriptionId) -> Sink<T> {
		let sink = self.subscriber.assign_id(id);
		Sink {
			sink: sink,
			_data: PhantomData,
		}
	}
}

pub struct Sink<T> {
	sink: pubsub::Sink,
	_data: PhantomData<T>,
}

impl<T: serde::Serialize> Sink<T> {
	pub fn send(&self, val: T) -> pubsub::SinkResult {
		let val = to_value(val);
		self.sink.send(core::Params::Array(vec![val]))
	}
}
