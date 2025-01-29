use std::sync::Arc;

use futures::channel::oneshot;

use crate::{
	packet::{ClosePacket, CloseReason},
	ws::WebSocketWrite,
	WispError,
};

use super::{StreamInfo, WsEvent};

/// Close handle for a multiplexor stream.
#[derive(Clone)]
pub struct MuxStreamCloser<W: WebSocketWrite> {
	pub(crate) info: Arc<StreamInfo>,
	pub(crate) inner: flume::Sender<WsEvent<W>>,
}

impl<W: WebSocketWrite + 'static> MuxStreamCloser<W> {
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
