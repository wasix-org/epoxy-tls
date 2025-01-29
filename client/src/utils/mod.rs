mod js;
mod rustls;
pub use js::*;
use js_sys::Uint8Array;
pub use rustls::*;
use wasm_streams::writable::IntoSink;
use wisp_mux::{ws::Payload, WispError};

use std::{
	pin::Pin,
	task::{Context, Poll},
};

use bytes::{buf::UninitSlice, BufMut, Bytes, BytesMut};
use futures_util::{ready, AsyncRead, Future, Sink, SinkExt, Stream};
use http::{HeaderValue, Uri};
use hyper::rt::Executor;
use pin_project_lite::pin_project;
use wasm_bindgen::prelude::*;

use crate::EpoxyError;

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(js_namespace = console, js_name = log)]
	pub fn js_console_log(s: &str);

	#[wasm_bindgen(js_namespace = console, js_name = warn)]
	pub fn js_console_warn(s: &str);

	#[wasm_bindgen(js_namespace = console, js_name = error)]
	pub fn js_console_error(s: &str);
}

#[macro_export]
macro_rules! console_log {
	($($expr:expr),*) => {
		$crate::utils::js_console_log(&format!($($expr),*))
	};
}
#[macro_export]
macro_rules! console_warn {
	($($expr:expr),*) => {
		$crate::utils::js_console_warn(&format!($($expr),*))
	};
}

#[macro_export]
macro_rules! console_error {
	($($expr:expr),*) => {
		$crate::utils::js_console_error(&format!($($expr),*))
	};
}

pub fn is_redirect(code: u16) -> bool {
	[301, 302, 303, 307, 308].contains(&code)
}

pub fn is_null_body(code: u16) -> bool {
	[101, 204, 205, 304].contains(&code)
}

pub trait UriExt {
	fn get_redirect(&self, location: &HeaderValue) -> Result<Uri, EpoxyError>;
}

impl UriExt for Uri {
	fn get_redirect(&self, location: &HeaderValue) -> Result<Uri, EpoxyError> {
		let new_uri = location.to_str()?.parse::<hyper::Uri>()?;
		let mut new_parts: http::uri::Parts = new_uri.into();
		if new_parts.scheme.is_none() {
			new_parts.scheme = self.scheme().cloned();
		}
		if new_parts.authority.is_none() {
			new_parts.authority = self.authority().cloned();
		}

		Ok(Uri::from_parts(new_parts)?)
	}
}

#[derive(Clone)]
pub struct WasmExecutor;

impl<F> Executor<F> for WasmExecutor
where
	F: Future + Send + 'static,
	F::Output: Send + 'static,
{
	fn execute(&self, future: F) {
		wasm_bindgen_futures::spawn_local(async move {
			let _ = future.await;
		});
	}
}

pin_project! {
	#[derive(Debug)]
	pub struct ReaderStream<R> {
		#[pin]
		reader: Option<R>,
		buf: BytesMut,
		capacity: usize,
	}
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

pub struct WispTransportWrite(pub IntoSink<'static>);
unsafe impl Send for WispTransportWrite {}

impl Sink<Payload> for WispTransportWrite {
	type Error = WispError;

	fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.0
			.poll_ready_unpin(cx)
			.map_err(|x| WispError::WsImplError(Box::new(EpoxyError::wisp_transport(x))))
	}

	fn start_send(mut self: Pin<&mut Self>, item: Payload) -> Result<(), Self::Error> {
		self.0
			.start_send_unpin(Uint8Array::from(item.as_ref()).into())
			.map_err(|x| WispError::WsImplError(Box::new(EpoxyError::wisp_transport(x))))
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.0
			.poll_flush_unpin(cx)
			.map_err(|x| WispError::WsImplError(Box::new(EpoxyError::wisp_transport(x))))
	}
	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.0
			.poll_close_unpin(cx)
			.map_err(|x| WispError::WsImplError(Box::new(EpoxyError::wisp_transport(x))))
	}
}
