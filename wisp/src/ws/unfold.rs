// Similar to `futures-util` `StreamExt::unfold` and `SinkExt::unfold`

use std::{
	future::Future,
	pin::Pin,
	task::{ready, Context, Poll},
};

use futures::{Sink, Stream};
use pin_project::pin_project;

use crate::WispError;

use super::Payload;

pub fn async_iterator_transport_read<State, Func, Fut>(
	init: State,
	func: Func,
) -> AsyncIteratorTransportRead<State, Func, Fut>
where
	Func: FnMut(State) -> Fut,
	Fut: Future<Output = Result<Option<(Payload, State)>, WispError>>,
{
	AsyncIteratorTransportRead {
		func,
		state: IteratorState::Value(init),
	}
}

pub fn async_iterator_transport_write<State, Func, Fut, CloseState, CloseFunc, CloseFut>(
	init: State,
	func: Func,
	close_init: CloseState,
	close_func: CloseFunc,
) -> AsyncIteratorTransportWrite<State, Func, Fut, CloseState, CloseFunc, CloseFut>
where
	Func: FnMut(State, Payload) -> Fut,
	Fut: Future<Output = Result<State, WispError>>,
	CloseFunc: FnMut(CloseState) -> CloseFut,
	CloseFut: Future<Output = Result<(), WispError>>,
{
	AsyncIteratorTransportWrite {
		func,
		state: IteratorState::Value(init),

		close: close_func,
		close_state: IteratorState::Value(close_init),
	}
}

#[pin_project(project = IteratorStateProj, project_replace = IteratorStateProjReplace)]
enum IteratorState<S, Fut> {
	Value(S),
	Future(#[pin] Fut),
	Empty,
}

impl<S, Fut> IteratorState<S, Fut> {
	pub fn take_state(self: Pin<&mut Self>) -> Option<S> {
		match &*self {
			Self::Value { .. } => match self.project_replace(Self::Empty) {
				IteratorStateProjReplace::Value(value) => Some(value),
				_ => unreachable!(),
			},
			_ => None,
		}
	}

	pub fn get_future(self: Pin<&mut Self>) -> Option<Pin<&mut Fut>> {
		match self.project() {
			IteratorStateProj::Future(future) => Some(future),
			_ => None,
		}
	}
}

#[pin_project]
pub struct AsyncIteratorTransportRead<State, Func, Fut>
where
	Func: FnMut(State) -> Fut,
	Fut: Future<Output = Result<Option<(Payload, State)>, WispError>>,
{
	func: Func,
	#[pin]
	state: IteratorState<State, Fut>,
}

impl<State, Func, Fut> Stream for AsyncIteratorTransportRead<State, Func, Fut>
where
	Func: FnMut(State) -> Fut,
	Fut: Future<Output = Result<Option<(Payload, State)>, WispError>>,
{
	type Item = Result<Payload, WispError>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = self.project();

		if let Some(state) = this.state.as_mut().take_state() {
			this.state.set(IteratorState::Future((this.func)(state)));
		}

		let ret = match this.state.as_mut().get_future() {
			Some(fut) => ready!(fut.poll(cx)),
			None => panic!("AsyncIteratorTransportRead was polled after completion"),
		};

		match ret {
			Ok(Some((ret, state))) => {
				this.state.set(IteratorState::Value(state));
				Poll::Ready(Some(Ok(ret)))
			}
			Ok(None) => Poll::Ready(None),
			Err(err) => Poll::Ready(Some(Err(err))),
		}
	}
}

#[pin_project]
pub struct AsyncIteratorTransportWrite<State, Func, Fut, CloseState, CloseFunc, CloseFut>
where
	Func: FnMut(State, Payload) -> Fut,
	Fut: Future<Output = Result<State, WispError>>,
	CloseFunc: FnMut(CloseState) -> CloseFut,
	CloseFut: Future<Output = Result<(), WispError>>,
{
	func: Func,
	#[pin]
	state: IteratorState<State, Fut>,

	close: CloseFunc,
	#[pin]
	close_state: IteratorState<CloseState, CloseFut>,
}

impl<State, Func, Fut, CloseState, CloseFunc, CloseFut> Sink<Payload>
	for AsyncIteratorTransportWrite<State, Func, Fut, CloseState, CloseFunc, CloseFut>
where
	Func: FnMut(State, Payload) -> Fut,
	Fut: Future<Output = Result<State, WispError>>,
	CloseFunc: FnMut(CloseState) -> CloseFut,
	CloseFut: Future<Output = Result<(), WispError>>,
{
	type Error = WispError;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.poll_flush(cx)
	}

	fn start_send(self: Pin<&mut Self>, item: Payload) -> Result<(), Self::Error> {
		let mut this = self.project();
		let fut = match this.state.as_mut().take_state() {
            Some(state) => (this.func)(state, item),
            None => panic!("start_send called on AsyncIteratorTransportWrite without poll_ready being called first"),
        };
		this.state.set(IteratorState::Future(fut));

		Ok(())
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		let mut this = self.project();
		Poll::Ready(if let Some(future) = this.state.as_mut().get_future() {
			match ready!(future.poll(cx)) {
				Ok(state) => {
					this.state.set(IteratorState::Value(state));
					Ok(())
				}
				Err(err) => {
					this.state.set(IteratorState::Empty);
					Err(err)
				}
			}
		} else {
			Ok(())
		})
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		ready!(self.as_mut().poll_flush(cx))?;
		let mut this = self.project();

		if let Some(future) = this.close_state.as_mut().get_future() {
			let ret = ready!(future.poll(cx));
			this.close_state.set(IteratorState::Empty);
			Poll::Ready(ret)
		} else {
			let future = match this.close_state.as_mut().take_state() {
				Some(value) => (this.close)(value),
				None => {
					panic!("poll_close called on AsyncIteratorTransportWrite after it finished")
				}
			};

			this.close_state.set(IteratorState::Future(future));
			Poll::Pending
		}
	}
}
