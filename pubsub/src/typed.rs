//! PUB-SUB auto-serializing structures.

use std::marker::PhantomData;
use std::pin::Pin;

use crate::subscription;
use crate::types::{SinkResult, SubscriptionId, TransportError};

use crate::core::futures::task::{Context, Poll};
use crate::core::futures::{self, channel};
use crate::core::{self, Error, Params, Value};

/// New PUB-SUB subscriber.
#[derive(Debug)]
pub struct Subscriber<T, E = Error> {
	subscriber: subscription::Subscriber,
	_data: PhantomData<(T, E)>,
}

impl<T, E> Subscriber<T, E> {
	/// Wrap non-typed subscriber.
	pub fn new(subscriber: subscription::Subscriber) -> Self {
		Subscriber {
			subscriber,
			_data: PhantomData,
		}
	}

	/// Create new subscriber for tests.
	pub fn new_test<M: Into<String>>(
		method: M,
	) -> (
		Self,
		crate::oneshot::Receiver<Result<SubscriptionId, Error>>,
		channel::mpsc::UnboundedReceiver<String>,
	) {
		let (subscriber, id, subscription) = subscription::Subscriber::new_test(method);
		(Subscriber::new(subscriber), id, subscription)
	}

	/// Reject subscription with given error.
	pub fn reject(self, error: Error) -> Result<(), ()> {
		self.subscriber.reject(error)
	}

	/// Reject subscription with given error.
	///
	/// The returned future will resolve when the response is sent to the client.
	pub async fn reject_async(self, error: Error) -> Result<(), ()> {
		self.subscriber.reject_async(error).await
	}

	/// Assign id to this subscriber.
	/// This method consumes `Subscriber` and returns `Sink`
	/// if the connection is still open or error otherwise.
	pub fn assign_id(self, id: SubscriptionId) -> Result<Sink<T, E>, ()> {
		let sink = self.subscriber.assign_id(id.clone())?;
		Ok(Sink {
			id,
			sink,
			_data: PhantomData,
		})
	}

	/// Assign id to this subscriber.
	/// This method consumes `Subscriber` and resolves to `Sink`
	/// if the connection is still open and the id has been sent or to error otherwise.
	pub async fn assign_id_async(self, id: SubscriptionId) -> Result<Sink<T, E>, ()> {
		let sink = self.subscriber.assign_id_async(id.clone()).await?;
		Ok(Sink {
			id,
			sink,
			_data: PhantomData,
		})
	}
}

/// Subscriber sink.
#[derive(Debug, Clone)]
pub struct Sink<T, E = Error> {
	sink: subscription::Sink,
	id: SubscriptionId,
	_data: PhantomData<(T, E)>,
}

impl<T: serde::Serialize, E: serde::Serialize> Sink<T, E> {
	/// Sends a notification to the subscriber.
	pub fn notify(&self, val: Result<T, E>) -> SinkResult {
		self.sink.notify(self.val_to_params(val))
	}

	fn to_value<V>(value: V) -> Value
	where
		V: serde::Serialize,
	{
		core::to_value(value).expect("Expected always-serializable type.")
	}

	fn val_to_params(&self, val: Result<T, E>) -> Params {
		let id = self.id.clone().into();
		let val = val.map(Self::to_value).map_err(Self::to_value);

		Params::Map(
			vec![
				("subscription".to_owned(), id),
				match val {
					Ok(val) => ("result".to_owned(), val),
					Err(err) => ("error".to_owned(), err),
				},
			]
			.into_iter()
			.collect(),
		)
	}
}

impl<T: serde::Serialize + Unpin, E: serde::Serialize + Unpin> futures::sink::Sink<Result<T, E>> for Sink<T, E> {
	type Error = TransportError;

	fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.sink).poll_ready(cx)
	}

	fn start_send(mut self: Pin<&mut Self>, item: Result<T, E>) -> Result<(), Self::Error> {
		let val = self.val_to_params(item);
		Pin::new(&mut self.sink).start_send(val)
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.sink).poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.sink).poll_close(cx)
	}
}
