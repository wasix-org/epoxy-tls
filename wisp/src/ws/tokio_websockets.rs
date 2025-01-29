use std::{pin::Pin, task::Poll};

use futures::{Sink, SinkExt, Stream, StreamExt};
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_websockets::{Message, WebSocketStream};

use crate::WispError;

use super::Payload;

#[pin_project]
pub struct TokioWebsocketsTransport<S: AsyncRead + AsyncWrite + Unpin>(
	#[pin] pub WebSocketStream<S>,
);

fn map_err(x: tokio_websockets::Error) -> WispError {
	if matches!(x, tokio_websockets::Error::AlreadyClosed) {
		WispError::WsImplSocketClosed
	} else {
		WispError::WsImplError(Box::new(x))
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> Stream for TokioWebsocketsTransport<S> {
	type Item = Result<Payload, WispError>;

	fn poll_next(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Option<Self::Item>> {
		match self.0.poll_next_unpin(cx) {
			Poll::Ready(Some(Ok(x))) => {
				if x.is_binary() {
					Poll::Ready(Some(Ok(x.into_payload().into())))
				} else if x.is_close() {
					Poll::Ready(None)
				} else {
					self.poll_next(cx)
				}
			}
			Poll::Ready(Some(Err(x))) => Poll::Ready(Some(Err(map_err(x)))),
			Poll::Ready(None) => Poll::Ready(None),
			Poll::Pending => Poll::Pending,
		}
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> Sink<Payload> for TokioWebsocketsTransport<S> {
	type Error = WispError;

	fn poll_ready(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_ready_unpin(cx).map_err(map_err)
	}

	fn start_send(mut self: Pin<&mut Self>, item: Payload) -> Result<(), Self::Error> {
		self.0
			.start_send_unpin(Message::binary(item))
			.map_err(map_err)
	}

	fn poll_flush(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_flush_unpin(cx).map_err(map_err)
	}

	fn poll_close(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_close_unpin(cx).map_err(map_err)
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> Sink<Message> for TokioWebsocketsTransport<S> {
	type Error = tokio_websockets::Error;

	fn poll_ready(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_ready_unpin(cx)
	}

	fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
		self.0.start_send_unpin(item)
	}

	fn poll_flush(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_flush_unpin(cx)
	}

	fn poll_close(
		mut self: Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.0.poll_close_unpin(cx)
	}
}
