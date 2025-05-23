use std::{
	marker::PhantomData,
	pin::Pin,
	task::{Context, Poll},
};

use futures::Sink;
use pin_project::pin_project;

#[pin_project]
pub struct SinkMap<Item, NewItem, S: Sink<Item>, F: FnMut(NewItem) -> Result<Item, S::Error>> {
	phantom: PhantomData<(Item, NewItem)>,
	#[pin]
	sink: S,
	func: F,
}

impl<Item, NewItem, S: Sink<Item>, F: FnMut(NewItem) -> Result<Item, S::Error>>
	SinkMap<Item, NewItem, S, F>
{
	pub fn new(sink: S, func: F) -> Self {
		Self {
			sink,
			func,
			phantom: PhantomData,
		}
	}
}

impl<Item, NewItem, S: Sink<Item>, F: FnMut(NewItem) -> Result<Item, S::Error>> Sink<NewItem>
	for SinkMap<Item, NewItem, S, F>
{
	type Error = S::Error;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().sink.poll_ready(cx)
	}

	fn start_send(self: Pin<&mut Self>, item: NewItem) -> Result<(), Self::Error> {
		let this = self.project();
		this.sink.start_send((this.func)(item)?)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().sink.poll_close(cx)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().sink.poll_flush(cx)
	}
}

pub trait SinkExtMap<Item>: Sink<Item> + Sized {
	fn map<NewItem, F: FnMut(NewItem) -> Result<Item, Self::Error>>(
		self,
		f: F,
	) -> SinkMap<Item, NewItem, Self, F> {
		SinkMap::new(self, f)
	}
}
impl<Item, T: Sink<Item>> SinkExtMap<Item> for T {}
