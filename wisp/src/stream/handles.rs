use std::sync::Arc;

use bytes::BufMut;
use futures::{channel::oneshot, SinkExt};

use crate::{
	locked_sink::LockedWebSocketWrite,
	packet::{ClosePacket, CloseReason},
	ws::{PayloadMut, PayloadRef, TransportWrite},
	WispError,
};

use super::{StreamInfo, WsEvent};

/// Close handle for a multiplexor stream.
#[derive(Clone)]
pub struct MuxStreamCloser<W: TransportWrite> {
	pub(crate) info: Arc<StreamInfo>,
	pub(crate) inner: flume::Sender<WsEvent<W>>,
}

impl<W: TransportWrite + 'static> MuxStreamCloser<W> {
	/// Close the stream. You will no longer be able to write or read after this has been called.
	pub async fn close(&self, reason: CloseReason) -> Result<(), WispError> {
		if self.inner.is_disconnected() {
			return Err(WispError::StreamAlreadyClosed);
		}

		let (tx, rx) = oneshot::channel::<Result<(), WispError>>();
		let evt = WsEvent::Close(self.info.id, ClosePacket { reason }, tx);

		self.inner
			.send_async(evt)
			.await
			.map_err(|_| WispError::MuxMessageFailedToSend)?;
		rx.await.map_err(|_| WispError::MuxMessageFailedToRecv)??;

		Ok(())
	}

	/// Get the stream's close reason, if it was closed.
	pub fn get_close_reason(&self) -> Option<CloseReason> {
		self.inner.is_disconnected().then(|| self.info.get_reason())
	}
}

/// Stream for sending arbitrary protocol extension packets.
pub struct MuxProtocolExtensionStream<W: TransportWrite> {
	pub(crate) info: Arc<StreamInfo>,
	pub(crate) tx: LockedWebSocketWrite<W>,
	pub(crate) inner: flume::Sender<WsEvent<W>>,
}

impl<W: TransportWrite> MuxProtocolExtensionStream<W> {
	/// Send a protocol extension packet with this stream's ID.
	pub async fn send(&mut self, packet_type: u8, data: PayloadRef<'_>) -> Result<(), WispError> {
		if self.inner.is_disconnected() {
			return Err(WispError::StreamAlreadyClosed);
		}

		let mut encoded = PayloadMut::with_capacity(1 + 4 + data.len());
		encoded.put_u8(packet_type);
		encoded.put_u32_le(self.info.id);
		encoded.extend(data.as_ref());

		self.tx.lock().await;
		let ret = self.tx.get().send(encoded.into()).await;
		self.tx.unlock();
		ret
	}
}
