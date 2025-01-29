use std::task::Poll;

use futures::{Sink, Stream};
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::WispError;

use super::Payload;

#[pin_project]
pub struct TokioTungsteniteTransport<S: AsyncRead + AsyncWrite + Unpin>(
	#[pin] pub WebSocketStream<S>,
);

fn map_err(x: tokio_tungstenite::tungstenite::Error) -> WispError {
	if matches!(x, tokio_tungstenite::tungstenite::Error::AlreadyClosed) {
		WispError::WsImplSocketClosed
	} else {
		WispError::WsImplError(Box::new(x))
	}
}

impl<S: AsyncRead + AsyncWrite + Unpin> Stream for TokioTungsteniteTransport<S> {
	type Item = Result<Payload, WispError>;

	fn poll_next(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Option<Self::Item>> {
		match self.as_mut().project().0.poll_next(cx) {
			Poll::Ready(Some(Ok(x))) => {
				if x.is_binary() {
					Poll::Ready(Some(Ok(x.into_data())))
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

impl<S: AsyncRead + AsyncWrite + Unpin> Sink<Payload> for TokioTungsteniteTransport<S> {
	type Error = WispError;

	fn poll_ready(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_ready(cx).map_err(map_err)
	}

	fn start_send(self: std::pin::Pin<&mut Self>, item: Payload) -> Result<(), Self::Error> {
		self.project()
			.0
			.start_send(Message::binary(item))
			.map_err(map_err)
	}

	fn poll_flush(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_flush(cx).map_err(map_err)
	}

	fn poll_close(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Result<(), Self::Error>> {
		self.project().0.poll_flush(cx).map_err(map_err)
	}
}
