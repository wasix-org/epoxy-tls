use std::sync::{Arc, Mutex, MutexGuard};

use futures::{Sink, SinkExt, Stream, StreamExt};

use super::{WebSocketRead, WebSocketWrite};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().expect("WebSocketSplit mutex was poisoned")
}

pub(crate) fn split<S: WebSocketRead + WebSocketWrite>(
	s: S,
) -> (WebSocketSplitRead<S>, WebSocketSplitWrite<S>) {
	let inner = Arc::new(Mutex::new(s));

	(
		WebSocketSplitRead(inner.clone()),
		WebSocketSplitWrite(inner),
	)
}

pub struct WebSocketSplitRead<S: WebSocketRead + WebSocketWrite>(Arc<Mutex<S>>);

impl<S: WebSocketRead + WebSocketWrite> Stream for WebSocketSplitRead<S> {
	type Item = S::Item;

	fn poll_next(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Option<Self::Item>> {
		lock(&self.0).poll_next_unpin(cx)
	}
}

pub struct WebSocketSplitWrite<S: WebSocketRead + WebSocketWrite>(Arc<Mutex<S>>);

impl<S: WebSocketRead + WebSocketWrite + Sink<T>, T> Sink<T> for WebSocketSplitWrite<S> {
	type Error = <S as Sink<T>>::Error;

	fn poll_ready(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		<S as SinkExt<T>>::poll_ready_unpin(&mut *lock(&self.0), cx)
	}

	fn start_send(self: std::pin::Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
		<S as SinkExt<T>>::start_send_unpin(&mut *lock(&self.0), item)
	}

	fn poll_flush(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		<S as SinkExt<T>>::poll_flush_unpin(&mut *lock(&self.0), cx)
	}

	fn poll_close(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		<S as SinkExt<T>>::poll_close_unpin(&mut *lock(&self.0), cx)
	}
}
