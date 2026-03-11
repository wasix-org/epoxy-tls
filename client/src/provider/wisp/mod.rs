use std::sync::Arc;

use bytes::Bytes;
use futures::{Sink, Stream, lock::Mutex};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::spawn_local;
use wisp_mux::{ClientMux, WispError, WispV2Handshake, packet::StreamType};

use crate::{
	EpoxyError,
	provider::service::{WasmProvider, WasmWispProvider},
	send_wrapper::SendWrapper,
};

use super::{
	ProviderServiceReq, ProviderUnencryptedStream,
	service::{BoxProviderService, ProviderService},
};

pub mod extension;

type WispProviderRead = Box<dyn Stream<Item = Result<Bytes, WispError>> + Send + Unpin>;
type WispProviderWrite = Box<dyn Sink<Bytes, Error = WispError> + Send + Unpin>;
pub struct WispProviderStream {
	pub read: WispProviderRead,
	pub write: WispProviderWrite,
}

struct WispProviderLocked {
	service: BoxProviderService<String, WispProviderStream, EpoxyError>,
	mux: Option<ClientMux<WispProviderWrite>>,
}

struct WispProviderInner {
	locked: Arc<Mutex<WispProviderLocked>>,

	server: String,
	v2: Box<dyn Fn() -> (Option<WispV2Handshake>, Vec<u8>) + Sync + Send>,
}

impl WispProviderInner {
	pub fn new(
		service: BoxProviderService<String, WispProviderStream, EpoxyError>,
		server: String,
	) -> Self {
		Self {
			locked: Arc::new(Mutex::new(WispProviderLocked { service, mux: None })),
			server,
			v2: Box::new(|| (None, vec![])),
		}
	}

	async fn create_mux(&self, guard: &mut WispProviderLocked) -> Result<(), EpoxyError> {
		let stream = guard.service.call(self.server.clone()).await?;
		let (v2, extensions) = (self.v2)();
		let (mux, fut) = ClientMux::new(stream.read, stream.write, v2)
			.await?
			.with_required_extensions(&extensions)
			.await?;

		spawn_local(async move {
			let _ = fut.await;
		});

		guard.mux.replace(mux);

		Ok(())
	}

	async fn call(&self, req: ProviderServiceReq) -> Result<ProviderUnencryptedStream, EpoxyError> {
		let mut guard = self.locked.clone().lock_owned().await;
		if guard.mux.is_none() {
			self.create_mux(&mut guard).await?;
		}
		let mux = guard.mux.as_mut().unwrap();

		let stream = mux.new_stream(StreamType::Tcp, req.host, req.port).await?;
		let (read, write) = stream.into_async_rw().into_split();

		Ok(ProviderUnencryptedStream {
			read: Box::new(read),
			write: Box::new(write),
		})
	}
}

#[wasm_bindgen]
pub struct WispProvider(Arc<WispProviderInner>);

#[wasm_bindgen]
impl WispProvider {
	pub fn new(provider: WasmWispProvider, server: String) -> WasmProvider {
		WasmProvider(BoxProviderService::new(Self(Arc::new(
			WispProviderInner::new(provider.0, server),
		))))
	}
}

impl ProviderService<ProviderServiceReq> for WispProvider {
	type Response = ProviderUnencryptedStream;
	type Error = EpoxyError;
	type Future = impl Future<Output = Result<Self::Response, Self::Error>> + Send;

	fn call(&self, req: ProviderServiceReq) -> Self::Future {
		let inner = self.0.clone();
		SendWrapper(async move { inner.call(req).await })
	}
}
