use std::ops::Deref;

use bytes::{Bytes, BytesMut};
use futures::{Sink, Stream, StreamExt};

use crate::WispError;

mod split;
pub use split::*;
mod unfold;
pub use unfold::*;

#[cfg(feature = "tokio-websockets")]
mod tokio_websockets;
#[cfg(feature = "tokio-websockets")]
pub use self::tokio_websockets::*;

#[cfg(feature = "tokio-tungstenite")]
mod tokio_tungstenite;
#[cfg(feature = "tokio-tungstenite")]
pub use self::tokio_tungstenite::*;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PayloadRef<'a> {
	Owned(Payload),
	Borrowed(&'a [u8]),
}

impl PayloadRef<'_> {
	pub fn into_owned(self) -> Payload {
		match self {
			Self::Owned(x) => x,
			Self::Borrowed(x) => BytesMut::from(x).freeze(),
		}
	}
}

impl From<Payload> for PayloadRef<'static> {
	fn from(value: Payload) -> Self {
		Self::Owned(value)
	}
}
impl<'a> From<&'a [u8]> for PayloadRef<'a> {
	fn from(value: &'a [u8]) -> Self {
		Self::Borrowed(value)
	}
}

impl Deref for PayloadRef<'_> {
	type Target = [u8];
	fn deref(&self) -> &Self::Target {
		match self {
			Self::Owned(x) => x,
			Self::Borrowed(x) => x,
		}
	}
}

pub type Payload = Bytes;
pub type PayloadMut = BytesMut;

pub trait TransportRead:
	Stream<Item = Result<Payload, WispError>> + Send + Unpin + 'static
{
}
impl<S: Stream<Item = Result<Payload, WispError>> + Send + Unpin + 'static> TransportRead for S {}

pub(crate) trait TransportReadExt: TransportRead {
	async fn next_erroring(&mut self) -> Result<Payload, WispError> {
		self.next().await.ok_or(WispError::WsImplSocketClosed)?
	}
}
impl<S: TransportRead> TransportReadExt for S {}

pub trait TransportWrite: Sink<Payload, Error = WispError> + Send + Unpin + 'static {}
impl<S: Sink<Payload, Error = WispError> + Send + Unpin + 'static> TransportWrite for S {}

pub trait TransportExt: TransportRead + TransportWrite + Sized {
	fn split_fast(self) -> (WebSocketSplitRead<Self>, WebSocketSplitWrite<Self>) {
		split::split(self)
	}
}
impl<S: TransportRead + TransportWrite + Sized> TransportExt for S {}
