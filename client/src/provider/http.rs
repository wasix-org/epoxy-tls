use bytes::Bytes;
use futures::{Stream, StreamExt};
use http_body::Frame;
use http_body_util::{Either, Empty, StreamBody};
use hyper::rt::Executor;
use hyper_util_wasm::client::legacy::Client;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{EpoxyError, EpoxyJsValErrorExt, send_wrapper::SendWrapper};

use super::StreamProviderService;

#[derive(Clone)]
pub struct WasmExecutor;
impl<T: Future<Output = ()> + 'static> Executor<T> for WasmExecutor {
	fn execute(&self, fut: T) {
		spawn_local(fut);
	}
}

pub struct EpoxyFrameStream(SendWrapper<wasm_streams::readable::IntoStream<'static>>);
impl EpoxyFrameStream {
	pub fn new(stream: wasm_streams::ReadableStream) -> Result<Self, EpoxyError> {
		match stream.try_into_stream() {
			Ok(x) => Ok(Self(SendWrapper(x))),
			Err((x, _)) => Err(x).js_error(),
		}
	}
}
impl Stream for EpoxyFrameStream {
	type Item = Result<Frame<Bytes>, EpoxyError>;

	fn poll_next(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Option<Self::Item>> {
		self.as_mut()
			.0
			.poll_next_unpin(cx)
			.map_ok(|x| {
				let u8array = x.unchecked_into::<Uint8Array>();
				Frame::data(Bytes::from(u8array.to_vec()))
			})
			.map_err(EpoxyError::js_error)
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.0.size_hint()
	}
}

pub type HyperClientBody = Either<Empty<Bytes>, StreamBody<EpoxyFrameStream>>;
pub type HyperClient = Client<StreamProviderService, HyperClientBody>;

pub fn build_hyper_body(body: Option<EpoxyFrameStream>) -> HyperClientBody {
	match body {
		Some(body) => Either::Right(StreamBody::new(body)),
		None => Either::Left(Empty::new()),
	}
}

pub fn build_hyper_client(provider: StreamProviderService) -> HyperClient {
	Client::builder(WasmExecutor)
		.http09_responses(true)
		.build(provider)
}
