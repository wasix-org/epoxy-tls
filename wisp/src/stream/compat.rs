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
	/// Writes a data packet for this stream onto the shared transport.
	///
	/// Every `Pending` from the sink goes back through `unlock_and_wait` rather than a
	/// plain unlock: the sink holds a single waker and the next task to lock it takes that
	/// waker away, so releasing the lock without queueing loses the wakeup outright. See
	/// `LockedSink::unlock_and_wait`.
	fn poll_write(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &[u8],
	) -> Poll<io::Result<usize>> {
		ready!(self.write.poll_lock(cx));

		let flushed = self.write.get().poll_flush(cx);
		match flushed {
			Poll::Pending => {
				self.write.unlock_and_wait(cx);
				return Poll::Pending;
			}
			Poll::Ready(Err(err)) => {
				self.write.unlock();
				return Poll::Ready(Err(io::Error::other(err)));
			}
			Poll::Ready(Ok(())) => {}
		}

		let readied = self.write.get().poll_ready(cx);
		match readied {
			Poll::Pending => {
				self.write.unlock_and_wait(cx);
				return Poll::Pending;
			}
			Poll::Ready(Err(err)) => {
				self.write.unlock();
				return Poll::Ready(Err(io::Error::other(err)));
			}
			Poll::Ready(Ok(())) => {}
		}

		let packet = Packet::new_data(self.info.id, buf);
		let sent = self.write.get().start_send(packet.encode());
		self.write.unlock();

		Poll::Ready(sent.map(|()| buf.len()).map_err(io::Error::other))
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		ready!(self.write.poll_lock(cx));

		let flushed = self.write.get().poll_flush(cx);
		match flushed {
			Poll::Pending => {
				self.write.unlock_and_wait(cx);
				Poll::Pending
			}
			Poll::Ready(res) => {
				self.write.unlock();
				Poll::Ready(res.map_err(io::Error::other))
			}
		}
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		if self.oneshot.is_none() {
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

			// Keep `oneshot` set only while a Close is genuinely in flight: if the send
			// failed, its sender is already gone, and leaving the receiver installed would
			// send a retried `poll_close` straight to awaiting a reply nobody can make.
			if self.inner.start_send_unpin(pkt).is_err() {
				self.oneshot = None;
				return Poll::Ready(Err(io::Error::other(WispError::MuxMessageFailedToSend)));
			}
		}

		// `flume::SendSink::start_send` only *stages* the item — it parks it in the
		// sink's hook and the send happens when the sink is next polled. So the close
		// has to be flushed, and the receiver has to be polled to register a waker, both
		// in this same call. Returning `Pending` right after `start_send`, which is what
		// this used to do, did neither: the actor was never handed the event and nothing
		// held a waker for the reply that would never come.
		//
		// Downstream that is why a socket never emitted `close`. `end` fired, the stream
		// layer auto-ended the writable side, `_final` called `writer.close()` — and that
		// promise never settled, so the writable never finished, autoDestroy never ran,
		// and every socket leaked until the worker exited.
		if ready!(self.inner.poll_flush_unpin(cx)).is_err() {
			self.oneshot = None;
			return Poll::Ready(Err(io::Error::other(WispError::MuxMessageFailedToSend)));
		}

		let oneshot = self
			.oneshot
			.as_mut()
			.expect("close oneshot was just installed");
		let ret = ready!(oneshot.poll_unpin(cx));
		self.oneshot = None;

		match ret.map_err(|_| io::Error::other(WispError::MuxMessageFailedToSend))? {
			// `InvalidStreamId` means the peer's CLOSE already took the stream out of the
			// mux, so the actor has no record of it — which is precisely the state this
			// close was asking for. Node does not fail `socket.end()` after a received FIN
			// either, and treating it as an error turns every `Connection: close` response
			// into a spurious socket `error` event.
			Ok(()) | Err(WispError::InvalidStreamId(_)) => Poll::Ready(Ok(())),
			Err(err) => Poll::Ready(Err(io::Error::other(err))),
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
