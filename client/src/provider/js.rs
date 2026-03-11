use bytes::Bytes;
use futures::{SinkExt, StreamExt, TryStreamExt};
use js_sys::{Array, Function, Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;
use wasm_streams::{ReadableStream, WritableStream};
use wisp_mux::WispError;

use crate::{
	EpoxyError, EpoxyJsErrorExt, jsval_debug,
	provider::service::{BoxProviderService, WasmProvider, WasmWispProvider},
	send_wrapper::SendWrapper,
	sink_map::SinkExtMap,
};

use super::{
	ProviderServiceReq, ProviderUnencryptedStream, service::ProviderService,
	wisp::WispProviderStream,
};

#[wasm_bindgen]
pub struct JsProvider {
	provider: SendWrapper<Function>,
}

#[wasm_bindgen]
impl JsProvider {
	fn new(func: Function) -> Self {
		Self {
			provider: SendWrapper(func),
		}
	}

	pub fn provider_wisp(func: Function) -> WasmWispProvider {
		WasmWispProvider(BoxProviderService::new(Self::new(func)))
	}

	pub fn provider(func: Function) -> WasmProvider {
		WasmProvider(BoxProviderService::new(Self::new(func)))
	}

	async fn map_result(val: JsValue) -> Result<(ReadableStream, WritableStream), EpoxyError> {
		let val = JsFuture::from(val.dyn_into::<Promise>().js_invalid()?)
			.await
			.js_error()?;
		let arr = val.dyn_into::<Array>().js_invalid()?;

		let read = ReadableStream::from_raw(arr.get(0).dyn_into().js_invalid()?);
		let write = WritableStream::from_raw(arr.get(1).dyn_into().js_invalid()?);

		Ok((read, write))
	}
}

impl ProviderService<String> for JsProvider {
	type Response = WispProviderStream;
	type Error = EpoxyError;
	type Future = impl Future<Output = Result<Self::Response, Self::Error>> + Send;

	fn call(&self, request: String) -> Self::Future {
		let ret = self.provider.0.call1(&JsValue::NULL, &request.into());

		SendWrapper(async move {
			let (read, write) = Self::map_result(ret.js_error()?).await?;
			Ok(WispProviderStream {
				read: Box::new(SendWrapper(
					read.try_into_stream()
						.map_err(|x| x.0)
						.js_error()?
						.map(|x| {
							Ok(
								x.map_err(|x| WispError::WsImplError(jsval_debug(x).into()))?
									.dyn_into::<Uint8Array>()
									.map_err(|x| {
										WispError::WsImplError(Box::new(
											EpoxyError::InvalidJsValue(jsval_debug(x)),
										))
									})?
									.to_vec()
									.into_iter()
									.collect(),
							)
						}),
				)),
				write: Box::new(SendWrapper(
					write
						.try_into_sink()
						.map_err(|x| x.0)
						.js_error()?
						.map(|x: Bytes| Ok(Uint8Array::from(&x[..]).into()))
						.sink_map_err(|x| WispError::WsImplError(format!("{x:?}").into())),
				)),
			})
		})
	}
}

impl ProviderService<ProviderServiceReq> for JsProvider {
	type Response = ProviderUnencryptedStream;
	type Error = EpoxyError;
	type Future = impl Future<Output = Result<Self::Response, Self::Error>> + Send;

	fn call(&self, request: ProviderServiceReq) -> Self::Future {
		let ret = self
			.provider
			.0
			.call2(&JsValue::NULL, &request.host.into(), &request.port.into());

		SendWrapper(async move {
			let (read, write) = Self::map_result(ret.js_error()?).await?;
			Ok(ProviderUnencryptedStream {
				read: Box::new(SendWrapper(
					read.try_into_stream()
						.map_err(|x| x.0)
						.js_error()?
						.map(|x| {
							Ok(x.js_error()
								.map_err(std::io::Error::other)?
								.dyn_into::<Uint8Array>()
								.js_invalid()
								.map_err(std::io::Error::other)?
								.to_vec())
						})
						.into_async_read(),
				)),
				write: Box::new(SendWrapper(
					write.try_into_async_write().map_err(|x| x.0).js_error()?,
				)),
			})
		})
	}
}
