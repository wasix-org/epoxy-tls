use std::{
	pin::Pin,
	task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite, Sink, Stream};
use pin_project::pin_project;

#[pin_project]
pub struct SendWrapper<T>(#[pin] pub T);
unsafe impl<T> Sync for SendWrapper<T> {}
unsafe impl<T> Send for SendWrapper<T> {}

impl<T: Future> Future for SendWrapper<T> {
	type Output = T::Output;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.project().0.poll(cx)
	}
}

impl<T: Stream> Stream for SendWrapper<T> {
	type Item = T::Item;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.project().0.poll_next(cx)
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.0.size_hint()
	}
}

impl<I, T: Sink<I>> Sink<I> for SendWrapper<T> {
	type Error = T::Error;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_ready(cx)
	}

	fn start_send(self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
		self.project().0.start_send(item)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_close(cx)
	}
}

impl<T: AsyncRead> AsyncRead for SendWrapper<T> {
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<std::io::Result<usize>> {
		self.project().0.poll_read(cx, buf)
	}

	fn poll_read_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &mut [std::io::IoSliceMut<'_>],
	) -> Poll<std::io::Result<usize>> {
		self.project().0.poll_read_vectored(cx, bufs)
	}
}

impl<T: AsyncWrite> AsyncWrite for SendWrapper<T> {
	fn poll_write(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<std::io::Result<usize>> {
		self.project().0.poll_write(cx, buf)
	}

	fn poll_write_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &[std::io::IoSlice<'_>],
	) -> Poll<std::io::Result<usize>> {
		self.project().0.poll_write_vectored(cx, bufs)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
		self.project().0.poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
		self.project().0.poll_close(cx)
	}
}
