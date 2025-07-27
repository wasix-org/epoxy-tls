use std::{
	pin::Pin,
	task::{Context, Poll},
};

use futures::{stream::FusedStream, Stream};
use pin_project::pin_project;

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum PollNext {
	/// Poll the first stream.
	Left,
	/// Poll the second stream.
	Right,
}

impl PollNext {
	/// Toggle the value and return the old one.
	pub fn toggle(&mut self) -> Self {
		let old = *self;
		*self = self.other();
		old
	}

	fn other(self) -> Self {
		match self {
			Self::Left => Self::Right,
			Self::Right => Self::Left,
		}
	}
}

pub fn unfair_select<S1, S2>(stream1: S1, stream2: S2) -> UnfairSelect<S1, S2>
where
	S1: Stream,
	S2: Stream<Item = S1::Item>,
{
	UnfairSelect {
		stream1,
		stream2,
		next: PollNext::Left,
		finished: false,
	}
}

#[pin_project(project = UnfairSelectProj)]
pub struct UnfairSelect<S1, S2> {
	#[pin]
	stream1: S1,
	#[pin]
	stream2: S2,

	next: PollNext,
	finished: bool,
}
impl<S1, S2> UnfairSelect<S1, S2> {
	/// Acquires a reference to the underlying streams that this combinator is
	/// pulling from.
	pub fn get_ref(&self) -> (&S1, &S2) {
		(&self.stream1, &self.stream2)
	}

	/// Acquires a mutable reference to the underlying streams that this
	/// combinator is pulling from.
	///
	/// Note that care must be taken to avoid tampering with the state of the
	/// stream which may otherwise confuse this combinator.
	pub fn get_mut(&mut self) -> (&mut S1, &mut S2) {
		(&mut self.stream1, &mut self.stream2)
	}

	/// Acquires a pinned mutable reference to the underlying streams that this
	/// combinator is pulling from.
	///
	/// Note that care must be taken to avoid tampering with the state of the
	/// stream which may otherwise confuse this combinator.
	pub fn get_pin_mut(self: Pin<&mut Self>) -> (Pin<&mut S1>, Pin<&mut S2>) {
		let this = self.project();
		(this.stream1, this.stream2)
	}

	/// Consumes this combinator, returning the underlying streams.
	///
	/// Note that this may discard intermediate state of this combinator, so
	/// care should be taken to avoid losing resources when this is called.
	pub fn into_inner(self) -> (S1, S2) {
		(self.stream1, self.stream2)
	}
}
impl<S1, S2> FusedStream for UnfairSelect<S1, S2>
where
	S1: Stream,
	S2: Stream<Item = S1::Item>,
{
	fn is_terminated(&self) -> bool {
		self.finished
	}
}
impl<S1, S2> Stream for UnfairSelect<S1, S2>
where
	S1: Stream,
	S2: Stream<Item = S1::Item>,
{
	type Item = S1::Item;

	fn poll_next(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Option<Self::Item>> {
		let mut this = self.project();
		let next = this.next.toggle();

		if *this.finished {
			Poll::Ready(None)
		} else {
			poll_inner(&mut this, next, cx)
		}
	}
}

#[inline]
fn poll_side<St1, St2>(
	select: &mut UnfairSelectProj<'_, St1, St2>,
	side: PollNext,
	cx: &mut Context<'_>,
) -> Poll<Option<St1::Item>>
where
	St1: Stream,
	St2: Stream<Item = St1::Item>,
{
	match side {
		PollNext::Left => select.stream1.as_mut().poll_next(cx),
		PollNext::Right => select.stream2.as_mut().poll_next(cx),
	}
}

#[inline]
fn poll_inner<St1, St2>(
	select: &mut UnfairSelectProj<'_, St1, St2>,
	side: PollNext,
	cx: &mut Context<'_>,
) -> Poll<Option<St1::Item>>
where
	St1: Stream,
	St2: Stream<Item = St1::Item>,
{
	match poll_side(select, side, cx) {
		Poll::Ready(Some(item)) => return Poll::Ready(Some(item)),
		Poll::Ready(None) => {
			*select.finished = true;
			return Poll::Ready(None);
		}
		Poll::Pending => false,
	};
	let other = side.other();
	match poll_side(select, other, cx) {
		Poll::Ready(None) => {
			*select.finished = true;
			Poll::Ready(None)
		}
		a => a,
	}
}
