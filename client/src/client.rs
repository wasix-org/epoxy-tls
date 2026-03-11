use futures::stream::{AbortHandle, Abortable};
use http::{HeaderName, HeaderValue, Method, Uri, request};
use http_body_util::BodyExt;
use hyper::{Request, Response, body::Incoming};
use tower::{Service, ServiceExt};
use wasm_bindgen::{
	JsCast,
	prelude::{Closure, wasm_bindgen},
};
use web_sys::AbortSignal;

use crate::{
	EpoxyError, console_log,
	provider::{
		StreamProvider,
		http::{
			EpoxyFrameStream, HyperClient, HyperClientBody, build_hyper_body, build_hyper_client,
		},
		service::WasmProvider,
	},
};

#[wasm_bindgen]
pub struct ClientReqBuilder(Option<request::Builder>);
#[wasm_bindgen]
impl ClientReqBuilder {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Self {
		Self(Some(request::Request::builder()))
	}

	fn modify(
		&mut self,
		func: impl FnOnce(request::Builder) -> Result<request::Builder, EpoxyError>,
	) -> Result<(), EpoxyError> {
		let take = self.0.take();
		self.0.replace(func(take.unwrap())?);

		Ok(())
	}

	pub fn method(&mut self, method: &str) -> Result<(), EpoxyError> {
		self.modify(|x| Ok(x.method(Method::try_from(method)?)))
	}

	pub fn uri(&mut self, uri: String) -> Result<(), EpoxyError> {
		self.modify(|x| Ok(x.uri(Uri::try_from(uri)?)))
	}

	pub fn header(&mut self, key: String, val: String) -> Result<(), EpoxyError> {
		self.modify(|x| Ok(x.header(HeaderName::try_from(key)?, HeaderValue::try_from(val)?)))
	}
}

#[wasm_bindgen]
pub struct Client {
	hyper: HyperClient,
}

#[wasm_bindgen]
impl Client {
	fn build_client(
		&self,
	) -> impl Service<
		Request<HyperClientBody>,
		Response = Response<Incoming>,
		Error = hyper_util_wasm::client::legacy::Error,
	> {
		self.hyper.clone()
	}

	#[wasm_bindgen(constructor)]
	pub fn new(backend: WasmProvider) -> Result<Self, EpoxyError> {
		let provider = StreamProvider::new(backend.0)?.into_service();

		Ok(Self {
			hyper: build_hyper_client(provider),
		})
	}

	async fn __request(
		&self,
		builder: ClientReqBuilder,
		body: Option<web_sys::ReadableStream>,
	) -> Result<(), EpoxyError> {
		let client = self.build_client();

		let body = build_hyper_body(
			body.map(|x| EpoxyFrameStream::new(wasm_streams::ReadableStream::from_raw(x)))
				.transpose()?,
		);
		let request = builder.0.unwrap().body(body)?;

		let response = client.oneshot(request).await?;

		let body = response.into_body().collect().await?;

		console_log!("resp {}", str::from_utf8(&body.to_bytes()).unwrap());

		Ok(())
	}

	pub async fn request(
		&self,
		builder: ClientReqBuilder,
		abort: AbortSignal,
		body: Option<web_sys::ReadableStream>,
	) -> Result<(), EpoxyError> {
		let (handle, reg) = AbortHandle::new_pair();
		let closure = Closure::<dyn Fn()>::new(move || handle.abort());
		abort.set_onabort(Some(closure.as_ref().unchecked_ref()));

		let ret = Abortable::new(self.__request(builder, body), reg).await;

		ret.map_err(|_| EpoxyError::Aborted).flatten()
	}
}
