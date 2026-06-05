use std::{
	pin::Pin,
	task::{Context, Poll, ready},
};

use bytes::{BufMut, Bytes, BytesMut, buf::UninitSlice};
use futures::{AsyncRead, AsyncWrite, AsyncWriteExt, Sink, SinkExt, Stream, TryStreamExt};
use js_sys::{Array, Uint8Array};
use pin_project::pin_project;
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_streams::{ReadableStream, WritableStream};

use crate::{EpoxyError, sink_map::SinkExtMap};

#[wasm_bindgen(typescript_custom_section)]
const JS_SOCKET_TS: &'static str = r#"
export type JsSocket = [ReadableStream<Uint8Array>, WritableStream<Uint8Array>];
export type JsTlsSocket = [ReadableStream<Uint8Array>, WritableStream<Uint8Array>, TlsStreamInfo];
"#;

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(typescript_type = "JsSocket")]
	pub type JsSocket;

	#[wasm_bindgen(typescript_type = "JsTlsSocket")]
	pub type JsTlsSocket;
}

/// Metadata about a negotiated TLS connection, handed back alongside the socket.
#[wasm_bindgen]
pub struct TlsStreamInfo {
	negotiated_protocol: Option<String>,
	protocol_version: Option<String>,
	cipher_suite: Option<String>,
	peer_certificates: Option<Vec<Vec<u8>>>,
}

impl TlsStreamInfo {
	pub fn new(
		negotiated_protocol: Option<String>,
		protocol_version: Option<String>,
		cipher_suite: Option<String>,
		peer_certificates: Option<Vec<Vec<u8>>>,
	) -> Self {
		Self {
			negotiated_protocol,
			protocol_version,
			cipher_suite,
			peer_certificates,
		}
	}
}

#[wasm_bindgen]
impl TlsStreamInfo {
	/// The ALPN protocol the server selected, if any.
	pub fn negotiated_protocol(&mut self) -> Option<String> {
		self.negotiated_protocol.take()
	}

	/// The negotiated TLS version (e.g. `TLSv1_3`), or its hex code if rustls
	/// doesn't have a name for it.
	pub fn protocol_version(&mut self) -> Option<String> {
		self.protocol_version.take()
	}

	/// The negotiated cipher suite's IANA name (e.g. `TLS13_AES_128_GCM_SHA256`),
	/// or its hex code if rustls doesn't have a name for it.
	pub fn cipher_suite(&mut self) -> Option<String> {
		self.cipher_suite.take()
	}

	/// The server's certificate chain as DER-encoded bytes, end-entity first.
	pub fn peer_certificates(&mut self) -> Option<Vec<Uint8Array>> {
		self.peer_certificates.take().map(|certs| {
			certs
				.into_iter()
				.map(|der| Uint8Array::from(der.as_slice()))
				.collect()
		})
	}
}

#[pin_project]
#[derive(Debug)]
pub struct ReaderStream<R> {
	#[pin]
	reader: Option<R>,
	buf: BytesMut,
	capacity: usize,
}

impl<R: AsyncRead> ReaderStream<R> {
	pub fn new(reader: R, capacity: usize) -> Self {
		ReaderStream {
			reader: Some(reader),
			buf: BytesMut::new(),
			capacity,
		}
	}
}

pub fn poll_read_buf<T: AsyncRead + ?Sized, B: BufMut>(
	io: Pin<&mut T>,
	cx: &mut Context<'_>,
	buf: &mut B,
) -> Poll<std::io::Result<usize>> {
	if !buf.has_remaining_mut() {
		return Poll::Ready(Ok(0));
	}

	let n = {
		let dst = buf.chunk_mut();

		let dst = unsafe { &mut *(std::ptr::from_mut::<UninitSlice>(dst) as *mut [u8]) };
		ready!(io.poll_read(cx, dst)?)
	};

	unsafe {
		buf.advance_mut(n);
	}

	Poll::Ready(Ok(n))
}

impl<R: AsyncRead> Stream for ReaderStream<R> {
	type Item = std::io::Result<Bytes>;
	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = self.as_mut().project();

		let Some(reader) = this.reader.as_pin_mut() else {
			return Poll::Ready(None);
		};

		if this.buf.capacity() == 0 {
			this.buf.reserve(*this.capacity);
		}

		match poll_read_buf(reader, cx, &mut this.buf) {
			Poll::Pending => Poll::Pending,
			Poll::Ready(Err(err)) => {
				self.project().reader.set(None);
				Poll::Ready(Some(Err(err)))
			}
			Poll::Ready(Ok(0)) => {
				self.project().reader.set(None);
				Poll::Ready(None)
			}
			Poll::Ready(Ok(_)) => {
				let chunk = this.buf.split();
				Poll::Ready(Some(Ok(chunk.freeze())))
			}
		}
	}
}

pub fn create_asyncread_js_socket(
	read: impl AsyncRead + 'static,
	buffer_size: usize,
	write: impl AsyncWrite + 'static,
) -> JsSocket {
	create_js_socket(
		ReaderStream::new(read, buffer_size).map_err(Into::into),
		write.into_sink().sink_map_err(Into::into),
	)
}
pub fn create_asyncread_js_tls_socket(
	read: impl AsyncRead + 'static,
	buffer_size: usize,
	write: impl AsyncWrite + 'static,
	info: TlsStreamInfo,
) -> JsTlsSocket {
	let arr = create_socket_array(
		ReaderStream::new(read, buffer_size).map_err(Into::into),
		write.into_sink().sink_map_err(Into::into),
	);
	arr.set(2, JsValue::from(info));
	JsValue::from(arr).into()
}
fn create_socket_array(
	stream: impl Stream<Item = Result<Bytes, EpoxyError>> + 'static,
	sink: impl Sink<Bytes, Error = EpoxyError> + 'static,
) -> Array {
	let arr = Array::new();
	let read = ReadableStream::from_stream(
		stream
			.map_ok(|x| Uint8Array::from(x.as_ref()).into())
			.map_err(Into::into),
	)
	.into_raw();
	let write = WritableStream::from_sink(
		sink.map(|x: JsValue| Ok(x.unchecked_into::<Uint8Array>().to_vec().into()))
			.sink_map_err(Into::into),
	)
	.into_raw();
	arr.set(0, read.into());
	arr.set(1, write.into());
	arr
}
pub fn create_js_socket(
	stream: impl Stream<Item = Result<Bytes, EpoxyError>> + 'static,
	sink: impl Sink<Bytes, Error = EpoxyError> + 'static,
) -> JsSocket {
	JsValue::from(create_socket_array(stream, sink)).into()
}
