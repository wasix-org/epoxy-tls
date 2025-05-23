use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming};
use js_sys::Function;
use tower::{Service, ServiceExt};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{
	EpoxyError, console_log,
	provider::{
		StreamProvider,
		http::{HyperClient, HyperClientBody, build_hyper_client},
		js::JsProvider,
		service::BoxProviderService,
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
	pub fn new(func: Function) -> Result<Self, EpoxyError> {
		let backend = BoxProviderService::new(JsProvider::new(func));

		let provider = StreamProvider::new(backend)?.into_service();

		Ok(Self {
			hyper: build_hyper_client(provider),
		})
	}

	pub async fn request(&self, url: String) -> Result<(), EpoxyError> {
		let mut client = self.build_client();
		client.ready().await?;

		let request = Request::builder().uri(url);

		let request = request.body(Full::new(Bytes::new()))?;

		let response = client.call(request).await?;

		let body = response.into_body().collect().await?;

		console_log!("resp {}", str::from_utf8(&body.to_bytes()).unwrap());

		Ok(())
	}
}
