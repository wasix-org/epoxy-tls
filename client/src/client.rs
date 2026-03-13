use futures::{
	TryStreamExt,
	stream::{AbortHandle, Abortable},
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, request};
use http_body_util::BodyExt;
use hyper::{Request, Response, body::Incoming};
use js_sys::{Array, Uint8Array};
use tower::{Service, ServiceExt};
use wasm_bindgen::{
	JsCast,
	prelude::{Closure, wasm_bindgen},
};
use web_sys::AbortSignal;

use crate::{
	EpoxyError,
	provider::{
		StreamProvider,
		http::{
			EpoxyFrameStream, HyperClient, HyperClientBody, build_hyper_body, build_hyper_client,
		},
		service::WasmProvider,
	},
};

#[wasm_bindgen]
pub struct Header {
	name: String,
	values: Vec<Uint8Array>,
}
#[wasm_bindgen]
impl Header {
	pub fn name(&mut self) -> String {
		std::mem::replace(&mut self.name, String::new())
	}

	pub fn values(&mut self) -> Vec<Uint8Array> {
		std::mem::replace(&mut self.values, Vec::new())
	}
}

#[wasm_bindgen]
pub struct ClientResponse {
	status: Option<StatusCode>,
	headers: Option<HeaderMap>,
	body: Option<Incoming>,
}
#[wasm_bindgen]
impl ClientResponse {
	pub fn status(&mut self) -> Array {
		let status = self.status.take().unwrap();
		Array::of2(&status.as_u16().into(), &status.as_str().into())
	}

	pub fn headers(&mut self) -> Vec<Header> {
		let mut headers = self.headers.take().unwrap();
		let mut out = Vec::with_capacity(headers.keys_len());

		let mut last_name = None;
		let mut values = vec![];
		for (name, value) in headers.drain() {
			if last_name.is_none()
				&& let Some(_name) = name
			{
				last_name = Some(_name);
			} else if let Some(_name) = name
				&& let Some(_last_name) = last_name
			{
				out.push(Header {
					name: _last_name.to_string(),
					values: std::mem::replace(&mut values, vec![]),
				});
				last_name = Some(_name);
			}
			values.push(Uint8Array::new_from_slice(value.as_bytes()));
		}

		out
	}

	pub fn body(&mut self) -> web_sys::ReadableStream {
		let body = self.body.take().unwrap();

		wasm_streams::ReadableStream::from_stream(
			body.into_data_stream()
				.map_ok(|x| Uint8Array::new_from_slice(&x).into())
				.map_err(|x| EpoxyError::from(x).into()),
		)
		.into_raw()
	}
}

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
	) -> Result<ClientResponse, EpoxyError> {
		let client = self.build_client();

		let body = build_hyper_body(
			body.map(|x| EpoxyFrameStream::new(wasm_streams::ReadableStream::from_raw(x)))
				.transpose()?,
		);
		let request = builder.0.unwrap().body(body)?;

		let response = client.oneshot(request).await?;
		let (parts, body) = response.into_parts();

		let res = ClientResponse {
			status: Some(parts.status),
			headers: Some(parts.headers),
			body: Some(body),
		};

		Ok(res)
	}

	pub async fn request(
		&self,
		builder: ClientReqBuilder,
		abort: AbortSignal,
		body: Option<web_sys::ReadableStream>,
	) -> Result<ClientResponse, EpoxyError> {
		let (handle, reg) = AbortHandle::new_pair();
		let closure = Closure::<dyn Fn()>::new(move || handle.abort());
		abort.set_onabort(Some(closure.as_ref().unchecked_ref()));

		let ret = Abortable::new(self.__request(builder, body), reg).await;

		ret.map_err(|_| EpoxyError::Aborted).flatten()
	}
}
