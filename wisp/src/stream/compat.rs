use std::{
	io,
	pin::Pin,
	sync::Arc,
	task::{ready, Context, Poll},
};

use futures::{
	channel::oneshot, stream::IntoAsyncRead, AsyncBufRead, AsyncRead, AsyncWrite, FutureExt,
	SinkExt, Stream, StreamExt, TryStreamExt,
};
use pin_project::pin_project;

use crate::{
	locked_sink::LockedWebSocketWrite,
	packet::{ClosePacket, CloseReason, Packet},
	ws::{Payload, TransportWrite},
	WispError,
};

use super::{MuxStream, MuxStreamRead, MuxStreamWrite, StreamInfo, WsEvent};

struct MapToIo<W: TransportWrite>(MuxStreamRead<W>);

impl<W: TransportWrite> Stream for MapToIo<W> {
	type Item = Result<Payload, std::io::Error>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.0.poll_next_unpin(cx).map_err(std::io::Error::other)
	}
}

// TODO: don't use `futures` for this so get_close_reason etc can be implemented
#[pin_project]
pub struct MuxStreamAsyncRead<W: TransportWrite> {
	#[pin]
	inner: IntoAsyncRead<MapToIo<W>>,
}

impl<W: TransportWrite> MuxStreamAsyncRead<W> {
	pub(crate) fn new(inner: MuxStreamRead<W>) -> Self {
		Self {
			inner: MapToIo(inner).into_async_read(),
		}
	}
}

impl<W: TransportWrite> AsyncRead for MuxStreamAsyncRead<W> {
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<io::Result<usize>> {
		self.project().inner.poll_read(cx, buf)
	}

	fn poll_read_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &mut [io::IoSliceMut<'_>],
	) -> Poll<io::Result<usize>> {
		self.project().inner.poll_read_vectored(cx, bufs)
	}
}

impl<W: TransportWrite> AsyncBufRead for MuxStreamAsyncRead<W> {
	fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
		self.project().inner.poll_fill_buf(cx)
	}

	fn consume(self: Pin<&mut Self>, amt: usize) {
		self.project().inner.consume(amt);
	}
}

pub struct MuxStreamAsyncWrite<W: TransportWrite> {
	inner: flume::r#async::SendSink<'static, WsEvent<W>>,
	write: LockedWebSocketWrite<W>,
	info: Arc<StreamInfo>,

	oneshot: Option<oneshot::Receiver<Result<(), WispError>>>,
}

impl<W: TransportWrite> MuxStreamAsyncWrite<W> {
	pub(crate) fn new(inner: MuxStreamWrite<W>) -> Self {
		Self {
			inner: inner.inner,
			write: inner.write,
			info: inner.info,

			oneshot: None,
		}
	}

	/// Get the stream's close reason, if it was closed.
	pub fn get_close_reason(&self) -> Option<CloseReason> {
		self.inner.is_disconnected().then(|| self.info.get_reason())
	}
}

impl<W: TransportWrite> AsyncWrite for MuxStreamAsyncWrite<W> {
	fn poll_write(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<io::Result<usize>> {
		ready!(self.write.poll_lock(cx));
		ready!(self.write.get().poll_flush(cx)).map_err(io::Error::other)?;
		ready!(self.write.get().poll_ready(cx)).map_err(io::Error::other)?;

		let packet = Packet::new_data(self.info.id, buf);
		self.write
			.get()
			.start_send(packet.encode())
			.map_err(io::Error::other)?;

		self.write.unlock();
		Poll::Ready(Ok(buf.len()))
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		ready!(self.write.poll_lock(cx));
		ready!(self.write.get().poll_flush(cx)).map_err(io::Error::other)?;
		self.write.unlock();
		Poll::Ready(Ok(()))
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		if let Some(oneshot) = &mut self.oneshot {
			let ret = ready!(oneshot.poll_unpin(cx));
			self.oneshot.take();
			Poll::Ready(
				ret.map_err(|_| io::Error::other(WispError::MuxMessageFailedToSend))?
					.map_err(io::Error::other),
			)
		} else {
			ready!(self.as_mut().poll_flush(cx))?;

			ready!(self.inner.poll_ready_unpin(cx))
				.map_err(|_| io::Error::other(WispError::MuxMessageFailedToSend))?;

			let (tx, rx) = oneshot::channel();
			self.oneshot = Some(rx);

			let pkt = WsEvent::Close(
				self.info.id,
				ClosePacket {
					reason: CloseReason::Unknown,
				},
				tx,
			);

			self.inner
				.start_send_unpin(pkt)
				.map_err(|_| io::Error::other(WispError::MuxMessageFailedToSend))?;

			Poll::Pending
		}
	}
}

#[pin_project]
pub struct MuxStreamAsyncRW<W: TransportWrite> {
	#[pin]
	read: MuxStreamAsyncRead<W>,
	#[pin]
	write: MuxStreamAsyncWrite<W>,
}

impl<W: TransportWrite> MuxStreamAsyncRW<W> {
	pub(crate) fn new(old: MuxStream<W>) -> Self {
		Self {
			read: MuxStreamAsyncRead::new(old.read),
			write: MuxStreamAsyncWrite::new(old.write),
		}
	}

	pub fn into_split(self) -> (MuxStreamAsyncRead<W>, MuxStreamAsyncWrite<W>) {
		(self.read, self.write)
	}

	/// Get the stream's close reason, if it was closed.
	pub fn get_close_reason(&self) -> Option<CloseReason> {
		self.write.get_close_reason()
	}
}

impl<W: TransportWrite> AsyncRead for MuxStreamAsyncRW<W> {
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<io::Result<usize>> {
		self.project().read.poll_read(cx, buf)
	}

	fn poll_read_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &mut [io::IoSliceMut<'_>],
	) -> Poll<io::Result<usize>> {
		self.project().read.poll_read_vectored(cx, bufs)
	}
}

impl<W: TransportWrite> AsyncBufRead for MuxStreamAsyncRW<W> {
	fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
		self.project().read.poll_fill_buf(cx)
	}

	fn consume(self: Pin<&mut Self>, amt: usize) {
		self.project().read.consume(amt);
	}
}

impl<W: TransportWrite> AsyncWrite for MuxStreamAsyncRW<W> {
	fn poll_write(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<io::Result<usize>> {
		self.project().write.poll_write(cx, buf)
	}

	fn poll_write_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &[io::IoSlice<'_>],
	) -> Poll<io::Result<usize>> {
		self.project().write.poll_write_vectored(cx, bufs)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		self.project().write.poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		self.project().write.poll_close(cx)
	}
}
