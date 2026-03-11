use futures::{SinkExt, StreamExt};
use js_sys::Uint8Array;
use wasm_bindgen::prelude::wasm_bindgen;
use wisp_mux::{extensions::ProtocolExtensionBuilder, ws::{Payload, TransportRead, TransportWrite}};

use crate::EpoxyError;

#[wasm_bindgen]
pub struct JsTransportRead {
	inner: *mut dyn TransportRead,
}
impl From<&mut dyn TransportRead> for JsTransportRead {
	fn from(value: &mut dyn TransportRead) -> Self {
	    Self { inner: &raw mut *value }
	}
}
#[wasm_bindgen]
impl JsTransportRead {
	pub async fn read(&mut self) -> Result<Option<Uint8Array>, EpoxyError> {
		let inner = unsafe { &mut *self.inner };
		let x = inner.next().await.transpose()?;
		Ok(x.map(|x| Uint8Array::from(x.as_ref())))
	}
}

#[wasm_bindgen]
pub struct JsTransportWrite {
	inner: *mut dyn TransportWrite,
}
impl From<&mut dyn TransportWrite> for JsTransportWrite {
	fn from(value: &mut dyn TransportWrite) -> Self {
	    Self { inner: &raw mut *value }
	}
}
#[wasm_bindgen]
impl JsTransportWrite {
	pub async fn write(&mut self, bytes: Uint8Array) -> Result<(), EpoxyError> {
		let inner = unsafe { &mut *self.inner };
		inner.send(Payload::from(bytes.to_vec())).await?;

		Ok(())
	}
}
