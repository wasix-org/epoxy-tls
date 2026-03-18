use std::{borrow::Cow, sync::Arc};

use futures::{
	AsyncReadExt, TryStreamExt,
	stream::{AbortHandle, Abortable},
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, request};
use http_body_util::BodyExt;
use hyper::{
	body::Incoming,
	ext::{HeaderCaseMap, ReasonPhrase},
};
use js_sys::{Array, Uint8Array};
use tower::{Service, ServiceBuilder, ServiceExt};
use wasm_bindgen::{
	JsCast,
	prelude::{Closure, wasm_bindgen},
};
use web_sys::AbortSignal;

use crate::{
	EpoxyError,
	http::{EpoxyBody, HyperClient, HyperRequest, HyperResponse, build_hyper_client},
	js_socket::{JsSocket, create_asyncread_js_socket},
	provider::{
		StreamProvider, StreamProviderService,
		service::WasmProvider,
	},
};

#[wasm_bindgen]
pub struct ClientReqBuilder {
	builder: Option<request::Builder>,
	case: HeaderCaseMap,
}
#[wasm_bindgen]
impl ClientReqBuilder {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Self {
		Self {
			builder: Some(request::Request::builder()),
			case: HeaderCaseMap::default(),
		}
	}

	fn modify(
		&mut self,
		func: impl FnOnce(request::Builder) -> Result<request::Builder, EpoxyError>,
	) -> Result<(), EpoxyError> {
		let take = self.builder.take();
		self.builder.replace(func(take.unwrap())?);

		Ok(())
	}

	pub fn method(&mut self, method: &str) -> Result<(), EpoxyError> {
		self.modify(|x| Ok(x.method(Method::try_from(method)?)))
	}

	pub fn uri(&mut self, uri: String) -> Result<(), EpoxyError> {
		self.modify(|x| Ok(x.uri(Uri::try_from(uri)?)))
	}

	pub fn header(&mut self, key: String, val: String) -> Result<(), EpoxyError> {
		let name = HeaderName::try_from(&key)?;
		self.case.append(name.clone(), key.into_bytes().into());
		self.modify(|x| Ok(x.header(name, HeaderValue::try_from(val)?)))
	}

	fn take(mut self) -> request::Builder {
		let builder = self.builder.take().unwrap();
		builder.extension(self.case)
	}
}

#[wasm_bindgen]
pub struct ClientResponse {
	status: Option<StatusCode>,
	status_text: Option<String>,
	headers: Option<HeaderMap>,
	header_case: Option<HeaderCaseMap>,
	body: Option<Incoming>,
}
#[wasm_bindgen]
impl ClientResponse {
	pub fn status(&mut self) -> u16 {
		let status = self.status.take().unwrap();
		status.as_u16()
	}

	pub fn status_text(&mut self) -> String {
		self.status_text.take().unwrap()
	}

	pub fn headers(&mut self) -> Vec<Array> {
		let headers = self.headers.take().unwrap();
		let header_case = self
			.header_case
			.take()
			.unwrap_or_else(|| HeaderCaseMap::default());
		let mut out = Vec::with_capacity(headers.len());

		for name in headers.keys() {
			let mut names = header_case.get_all_internal(name);

			for value in headers.get_all(name) {
				let name = names
					.next()
					.map(|x| String::from_utf8_lossy(x))
					.unwrap_or_else(|| Cow::Borrowed(name.as_str()));

				let arr = Array::new();
				arr.set(0, name.as_ref().into());
				arr.set(1, Uint8Array::new_from_slice(value.as_bytes()).into());
				out.push(arr);
			}
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
pub struct Client {
	provider: Arc<StreamProvider>,
	hyper: HyperClient,
}

#[wasm_bindgen]
impl Client {
	fn build_client(
		&self,
	) -> impl Service<HyperRequest, Response = HyperResponse, Error = EpoxyError> {
		ServiceBuilder::new().service(self.hyper.clone())
	}

	#[wasm_bindgen(constructor)]
	pub fn new(backend: WasmProvider) -> Result<Self, EpoxyError> {
		let provider = Arc::new(StreamProvider::new(backend.0)?);

		Ok(Self {
			hyper: build_hyper_client(StreamProviderService(provider.clone())),
			provider,
		})
	}

	async fn __request(
		&self,
		builder: ClientReqBuilder,
		body: Option<web_sys::ReadableStream>,
		length: Option<u64>,
	) -> Result<ClientResponse, EpoxyError> {
		let body = body
			.map(|x| EpoxyBody::new(x, length))
			.unwrap_or(EpoxyBody::Empty);
		let request = builder.take().body(body)?;
		let client = self.build_client();
		let response = client.oneshot(request).await?;

		let (mut parts, body) = response.into_parts();

		let status_text = parts
			.extensions
			.get::<ReasonPhrase>()
			.map(|x| String::from_utf8_lossy(x.as_bytes()).to_string())
			.or((parts.version > Version::HTTP_11).then(|| String::new())) // should this be here?
			.unwrap_or(parts.status.canonical_reason().unwrap_or("").to_string());

		let res = ClientResponse {
			status: Some(parts.status),
			status_text: Some(status_text),
			headers: Some(parts.headers),
			header_case: parts.extensions.remove::<HeaderCaseMap>(),
			body: Some(body),
		};

		Ok(res)
	}

	pub async fn request(
		&self,
		builder: ClientReqBuilder,
		abort: AbortSignal,
		body: Option<web_sys::ReadableStream>,
		length: Option<u64>,
	) -> Result<ClientResponse, EpoxyError> {
		let (handle, reg) = AbortHandle::new_pair();
		let closure = Closure::<dyn Fn()>::new(move || handle.abort());
		abort.set_onabort(Some(closure.as_ref().unchecked_ref()));

		let ret = Abortable::new(self.__request(builder, body, length), reg).await;

		ret.map_err(|_| EpoxyError::Aborted).flatten()
	}

	pub async fn connect(
		&self,
		host: String,
		port: u16,
		buffer_size: usize,
	) -> Result<JsSocket, EpoxyError> {
		self.provider
			.get_stream(host, port)
			.await
			.map(|x| create_asyncread_js_socket(x.read, buffer_size, x.write))
	}

	pub async fn connect_tls(
		&self,
		host: String,
		port: u16,
		buffer_size: usize,
	) -> Result<JsSocket, EpoxyError> {
		self.provider
			.get_tls_stream(host, port, false)
			.await
			.map(|x| {
				let (rx, tx) = x.stream.split();
				create_asyncread_js_socket(rx, buffer_size, tx)
			})
	}
}
