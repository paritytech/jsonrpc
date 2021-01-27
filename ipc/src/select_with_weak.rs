use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::stream::{Fuse, Stream};

pub trait SelectWithWeakExt: Stream {
	fn select_with_weak<S>(self, other: S) -> SelectWithWeak<Self, S>
	where
		S: Stream<Item = Self::Item>,
		Self: Sized;
}

impl<T> SelectWithWeakExt for T
where
	T: Stream,
{
	fn select_with_weak<S>(self, other: S) -> SelectWithWeak<Self, S>
	where
		S: Stream<Item = Self::Item>,
		Self: Sized,
	{
		new(self, other)
	}
}

/// An adapter for merging the output of two streams.
///
/// The merged stream produces items from either of the underlying streams as
/// they become available, and the streams are polled in a round-robin fashion.
/// Errors, however, are not merged: you get at most one error at a time.
///
/// Finishes when strong stream finishes
#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct SelectWithWeak<S1, S2> {
	strong: Fuse<S1>,
	weak: Fuse<S2>,
	use_strong: bool,
}

fn new<S1, S2>(stream1: S1, stream2: S2) -> SelectWithWeak<S1, S2>
where
	S1: Stream,
	S2: Stream<Item = S1::Item>,
{
	use futures::StreamExt;
	SelectWithWeak {
		strong: stream1.fuse(),
		weak: stream2.fuse(),
		use_strong: false,
	}
}

impl<S1, S2> Stream for SelectWithWeak<S1, S2>
where
	S1: Stream + Unpin,
	S2: Stream<Item = S1::Item> + Unpin,
{
	type Item = S1::Item;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = Pin::into_inner(self);
		let mut checked_strong = false;
		loop {
			if this.use_strong {
				match Pin::new(&mut this.strong).poll_next(cx) {
					Poll::Ready(Some(item)) => {
						this.use_strong = false;
						return Poll::Ready(Some(item));
					}
					Poll::Ready(None) => return Poll::Ready(None),
					Poll::Pending => {
						if !checked_strong {
							this.use_strong = false;
						} else {
							return Poll::Pending;
						}
					}
				}
				checked_strong = true;
			} else {
				this.use_strong = true;
				match Pin::new(&mut this.weak).poll_next(cx) {
					Poll::Ready(Some(item)) => return Poll::Ready(Some(item)),
					Poll::Ready(None) | Poll::Pending => (),
				}
			}
		}
	}
}
