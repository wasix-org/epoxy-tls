use std::{borrow::Cow, convert::Infallible, sync::Arc};

use futures::{
	AsyncReadExt, TryStreamExt,
	stream::{AbortHandle, Abortable},
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, header, request};
use http_body_util::BodyExt;
use hyper::{
	body::Incoming,
	ext::{HeaderCaseMap, ReasonPhrase},
};
use js_sys::{Array, Uint8Array};
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_http::{
	follow_redirect::{self, FollowRedirectLayer, RequestUri},
	set_header::SetRequestHeaderLayer,
};
use wasm_bindgen::{
	JsCast,
	prelude::{Closure, wasm_bindgen},
};
use web_sys::AbortSignal;

use crate::{
	EpoxyError,
	http::{EpoxyBody, HyperClient, HyperRequest, HyperResponse, build_hyper_client},
	js_socket::{JsSocket, create_asyncread_js_socket},
	provider::{StreamProvider, StreamProviderService, service::WasmProvider},
};
#[wasm_bindgen]
pub enum Redirect {
	Follow,
	Manual,
	Error,
}
#[derive(Clone)]
pub enum RedirectPolicy {
	Follow {
		remaining: usize,
		filter: follow_redirect::policy::FilterCredentials,
	},
	Manual,
	Error,
}
impl Redirect {
	pub fn into_policy(self) -> RedirectPolicy {
		match self {
			Self::Follow => RedirectPolicy::Follow {
				remaining: 20,
				filter: follow_redirect::policy::FilterCredentials::new(),
			},
			Self::Manual => RedirectPolicy::Manual,
			Self::Error => RedirectPolicy::Error,
		}
	}
}
impl follow_redirect::policy::Policy<EpoxyBody, EpoxyError> for RedirectPolicy {
	fn redirect(
		&mut self,
		attempt: &follow_redirect::policy::Attempt<'_>,
	) -> Result<follow_redirect::policy::Action, EpoxyError> {
		match self {
			Self::Follow { remaining, filter } => {
				let _ = follow_redirect::policy::Policy::<EpoxyBody, Infallible>::redirect(
					filter, attempt,
				); // always returns Ok(Follow)

				if *remaining > 0 {
					*remaining -= 1;
					Ok(follow_redirect::policy::Action::Follow)
				} else {
					Err(EpoxyError::TooManyRedirects)
				}
			}
			Self::Manual => Ok(follow_redirect::policy::Action::Stop),
			Self::Error => Err(EpoxyError::TooManyRedirects),
		}
	}

	fn on_request(&mut self, request: &mut http::Request<EpoxyBody>) {
		if let RedirectPolicy::Follow { filter, .. } = self {
			follow_redirect::policy::Policy::<EpoxyBody, Infallible>::on_request(filter, request);
		}
	}

	fn clone_body(&self, body: &EpoxyBody) -> Option<EpoxyBody> {
		Some(body.clone())
	}
}

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
	uri: Option<Uri>,
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

	pub fn uri(&mut self) -> String {
		self.uri.take().unwrap().to_string()
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
	ua: HeaderValue,
}

#[wasm_bindgen]
impl Client {
	fn build_client(
		&self,
		redirect: Redirect,
	) -> impl Service<HyperRequest, Response = HyperResponse, Error = EpoxyError> {
		ServiceBuilder::new()
			.layer(FollowRedirectLayer::with_policy(redirect.into_policy()))
			.layer(SetRequestHeaderLayer::if_not_present(
				header::ACCEPT,
				HeaderValue::from_static("*/*"),
			))
			.layer(SetRequestHeaderLayer::if_not_present(
				header::USER_AGENT,
				self.ua.clone(),
			))
			.service(self.hyper.clone())
	}

	#[wasm_bindgen(constructor)]
	pub fn new(backend: WasmProvider, ua: String) -> Result<Self, EpoxyError> {
		let provider = Arc::new(StreamProvider::new(backend.0)?);

		Ok(Self {
			hyper: build_hyper_client(StreamProviderService(provider.clone())),
			provider,
			ua: HeaderValue::from_str(&ua)?,
		})
	}

	pub fn set_ua(&mut self, ua: String) -> Result<(), EpoxyError> {
		self.ua = HeaderValue::from_str(&ua)?;

		Ok(())
	}
	pub fn get_ua(&mut self) -> String {
		// can only be set via a string
		self.ua.to_str().unwrap().to_string()
	}

	async fn __request(
		&self,
		builder: ClientReqBuilder,
		redirect: Redirect,
		body: Option<web_sys::ReadableStream>,
		length: Option<u64>,
	) -> Result<ClientResponse, EpoxyError> {
		let body = body
			.map(|x| EpoxyBody::new(x, length))
			.unwrap_or(EpoxyBody::Empty);
		let request = builder.take().body(body)?;
		let client = self.build_client(redirect);
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
			uri: Some(parts.extensions.remove::<RequestUri>().unwrap().0),
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
		redirect: Redirect,
		body: Option<web_sys::ReadableStream>,
		length: Option<u64>,
	) -> Result<ClientResponse, EpoxyError> {
		let (handle, reg) = AbortHandle::new_pair();
		let closure = Closure::<dyn Fn()>::new(move || handle.abort());
		abort.set_onabort(Some(closure.as_ref().unchecked_ref()));

		let ret = Abortable::new(self.__request(builder, redirect, body, length), reg).await;

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
