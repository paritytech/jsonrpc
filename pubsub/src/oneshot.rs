//! A futures oneshot channel that can be used for rendezvous.

use std::ops::{Deref, DerefMut};
use crate::core::futures::{self, Future, future, sync::oneshot};

/// Create a new future-base rendezvouz channel.
///
/// The returned `Sender` and `Receiver` objects are wrapping
/// the regular `futures::sync::oneshot` counterparts and have the same functionality.
/// Additionaly `Sender::send_and_wait` allows you to send a message to the channel
/// and get a future that resolves when the message is consumed.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
	let (sender, receiver) = oneshot::channel();
	let (receipt_tx, receipt_rx) = oneshot::channel();

	(
		Sender { sender, receipt: receipt_tx },
		Receiver { receiver, receipt: Some(receipt_rx) }
	)
}

/// A sender part of the channel.
#[derive(Debug)]
pub struct Sender<T> {
	sender: oneshot::Sender<T>,
	receipt: oneshot::Sender<()>,
}

impl<T> Sender<T> {
	/// Consume the sender and queue up an item to send.
	///
	/// This method returns right away and never blocks,
	/// there is no guarantee though that the message is received
	/// by the other end.
	pub fn send(self, t: T) -> Result<(), T> {
		self.sender.send(t)
	}

	/// Consume the sender and send an item.
	///
	/// The returned future will resolve when the message is received
	/// on the other end. Note that polling the future is actually not required
	/// to send the message as that happens synchronously.
	pub fn send_and_wait(self, t: T) -> impl Future<Item = (), Error = T> {
		let Self { sender, mut receipt } = self;

		if let Err(e) = sender.send(t) {
			return future::Either::A(future::err(e))
		}

		future::Either::B(future::poll_fn(
			move || Ok(receipt.poll_cancel().expect("poll_cancel never errors; qed"))
		))
	}
}

impl<T> Deref for Sender<T> {
	type Target = oneshot::Sender<T>;

	fn deref(&self) -> &Self::Target {
		&self.sender
	}
}

impl<T> DerefMut for Sender<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.sender
	}
}

/// Receiving end of the channel.
///
/// When this object is `polled` and the result is `Ready`
/// the other end (`Sender`) is also notified about the fact
/// that the item has been consumed and the future returned
/// by `send_and_wait` resolves.
#[must_use = "futures do nothing unless polled"]
#[derive(Debug)]
pub struct Receiver<T> {
	receiver: oneshot::Receiver<T>,
	receipt: Option<oneshot::Receiver<()>>,
}

impl<T> AsMut<oneshot::Receiver<T>> for Receiver<T> {
	fn as_mut(&mut self) -> &mut oneshot::Receiver<T> {
		&mut self.receiver
	}
}

impl<T> Future for Receiver<T> {
	type Item = <oneshot::Receiver<T> as Future>::Item;
	type Error = <oneshot::Receiver<T> as Future>::Error;

	fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
		match self.receiver.poll() {
			Ok(futures::Async::Ready(r)) => {
				self.receipt.take();
				Ok(futures::Async::Ready(r))
			},
			e => e,
		}
	}
}
