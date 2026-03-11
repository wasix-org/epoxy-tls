use bytes::Bytes;
use futures::stream::{AbortHandle, Abortable};
use http_body_util::{BodyExt, Full};
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
		http::{HyperClient, HyperClientBody, build_hyper_client},
		service::WasmProvider,
	},
};

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

	async fn __request(&self, url: String) -> Result<(), EpoxyError> {
		let client = self.build_client();

		let request = Request::builder().uri(url);

		let request = request.body(Full::new(Bytes::new()))?;

		let response = client.oneshot(request).await?;

		let body = response.into_body().collect().await?;

		console_log!("resp {}", str::from_utf8(&body.to_bytes()).unwrap());

		Ok(())
	}

	pub async fn request(&self, url: String, abort: AbortSignal) -> Result<(), EpoxyError> {
		let (handle, reg) = AbortHandle::new_pair();
		let closure = Closure::<dyn Fn()>::new(move || handle.abort());
		abort.set_onabort(Some(closure.as_ref().unchecked_ref()));
		let ret = Abortable::new(self.__request(url), reg).await;
		ret.map_err(|_| EpoxyError::Aborted).flatten()
	}
}
