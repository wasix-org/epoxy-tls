use std::sync::Arc;

use arti_client::{TorClient, TorClientConfig, config::BoolOrAuto};
use futures::AsyncReadExt;
use tor_config_path::CfgPath;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{
	EpoxyError,
	provider::{
		ProviderServiceReq, ProviderUnencryptedStream,
		service::{BoxProviderService, ProviderService, WasmProvider},
		tor::{
			dir_store::WasmDirStoreBuilder, runtime::WasmTorRuntime,
			state_mgr::WasmStateMgrBuilder,
		},
	},
	send_wrapper::SendWrapper,
};

mod dir_store;
pub mod js_types;
mod runtime;
mod state_mgr;

#[wasm_bindgen]
pub struct TorSocketProvider {
	inner: Arc<TorSocketProviderInner>,
}

struct TorSocketProviderInner {
	client: SendWrapper<TorClient<WasmTorRuntime>>,
}

#[wasm_bindgen]
impl TorSocketProvider {
	/// Construct a new `TorSocketProvider`.
	///
	/// `socket` is the underlying transport `WasmProvider` that the embedded
	/// `arti-client` will use for TCP connections to guards. Pass `None` to
	/// signal "snowflake" mode, which is not yet implemented.
	///
	/// `state` is the JS-backed persistent state manager — see
	/// `TorStateMgrCallbacks` in the TypeScript bindings.
	#[wasm_bindgen(constructor)]
	pub fn new(
		socket: Option<WasmProvider>,
		state: js_types::JsStateMgrCallbacks,
	) -> Result<Self, EpoxyError> {
		let socket = socket.ok_or_else(|| {
			EpoxyError::Unknown(
				"snowflake transport is not yet implemented; pass a SocketProvider"
					.to_string()
					.into(),
			)
		})?;

		let state_mgr = state_mgr::JsStateMgr::from_callbacks(state)?;

		let runtime = WasmTorRuntime::new(socket.0)?;
		let builder = WasmStateMgrBuilder::new(state_mgr);

		// `${ARTI_LOCAL_DATA}` and `${ARTI_CACHE}` can't be resolved on
		// `wasm32-unknown-unknown` (no real filesystem). Pin the dirs to literal
		// paths — our `JsStateMgr` is what actually persists data, and the
		// in-memory dir cache will be rebuilt on each fresh load.
		let mut config_builder = TorClientConfig::builder();
		config_builder
			.storage()
			.state_dir(CfgPath::new_literal("/wasm/state"))
			.cache_dir(CfgPath::new_literal("/wasm/cache"));
		// The keystore defaults to Native, which calls
		// `ArtiNativeKeystore::from_path_and_mistrust(state_dir.join("keystore"))`
		// — that fails on wasm because the path isn't real. Disable it; we
		// don't run hidden services or other key-using code from this client.
		config_builder
			.storage()
			.keystore()
			.enabled(BoolOrAuto::Explicit(false));
		let config = config_builder
			.build()
			.map_err(|e| EpoxyError::Unknown(e.into()))?;

		let client = TorClient::with_runtime(runtime)
			.config(config)
			.state_mgr_builder(Arc::new(builder))
			.dirmgr_store_builder(Arc::new(WasmDirStoreBuilder))
			.create_unbootstrapped()
			.map_err(|e| EpoxyError::Unknown(e.into()))?;

		Ok(Self {
			inner: Arc::new(TorSocketProviderInner {
				client: SendWrapper(client),
			}),
		})
	}

	/// Eagerly bootstrap the embedded `TorClient`.
	///
	/// If you skip this, the first `connect` will block until bootstrap
	/// finishes (the `BootstrapBehavior::OnDemand` default).
	pub async fn bootstrap(&self) -> Result<(), EpoxyError> {
		let inner = self.inner.clone();
		SendWrapper(async move {
			inner
				.client
				.0
				.bootstrap()
				.await
				.map_err(|e| EpoxyError::Unknown(e.into()))
		})
		.await
	}

	/// Box this provider as a `WasmProvider` so it can be passed wherever a
	/// `SocketProvider` is expected.
	pub fn r#box(self) -> WasmProvider {
		WasmProvider(BoxProviderService::new(TorSocketProviderService {
			inner: self.inner,
		}))
	}
}

#[derive(Clone)]
struct TorSocketProviderService {
	inner: Arc<TorSocketProviderInner>,
}

impl ProviderService<ProviderServiceReq> for TorSocketProviderService {
	type Response = ProviderUnencryptedStream;
	type Error = EpoxyError;
	type Future = impl Future<Output = Result<Self::Response, Self::Error>> + Send;

	fn call(&self, request: ProviderServiceReq) -> Self::Future {
		let inner = self.inner.clone();
		SendWrapper(async move {
			let stream = inner
				.client
				.0
				.connect((request.host.as_str(), request.port))
				.await
				.map_err(|e| EpoxyError::Unknown(e.into()))?;

			let (read, write) = AsyncReadExt::split(stream);
			Ok(ProviderUnencryptedStream {
				read: Box::new(SendWrapper(read)),
				write: Box::new(SendWrapper(write)),
			})
		})
	}
}
