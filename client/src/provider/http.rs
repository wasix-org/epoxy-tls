use bytes::Bytes;
use http_body_util::Full;
use hyper::rt::Executor;
use hyper_util_wasm::client::legacy::Client;
use wasm_bindgen_futures::spawn_local;

use super::StreamProviderService;

#[derive(Clone)]
pub struct WasmExecutor;
impl<T: Future<Output = ()> + 'static> Executor<T> for WasmExecutor {
	fn execute(&self, fut: T) {
		spawn_local(fut);
	}
}

pub type HyperClientBody = Full<Bytes>;
pub type HyperClient = Client<StreamProviderService, HyperClientBody>;

pub fn build_hyper_client(provider: StreamProviderService) -> HyperClient {
	Client::builder(WasmExecutor)
		.http09_responses(true)
		.build(provider)
}
