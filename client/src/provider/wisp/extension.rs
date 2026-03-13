use js_sys::{Array, Function, Promise};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;
use wisp_mux::{
	WispError, WispV2Handshake,
	extensions::{AnyProtocolExtension, AnyProtocolExtensionBuilder},
};

use crate::{
	EpoxyError,
	provider::wisp::js_extension::{
		JsProtocolExtension,
	},
	send_wrapper::SendWrapper,
};

use super::js_extension::JsProtocolExtensionBuilder;

#[macro_export]
macro_rules! refstruct {
	($name:ty, $refname:ident) => {
		#[wasm_bindgen::prelude::wasm_bindgen]
		pub struct $refname {
			inner: *mut $name,
		}
		impl From<&mut $name> for $refname {
			fn from(value: &mut $name) -> Self {
				Self {
					inner: &raw mut *value,
				}
			}
		}
		impl $refname {
			#[allow(dead_code)]
			fn inner(&self) -> &mut $name {
				unsafe { &mut *self.inner }
			}
		}
	};
}

#[wasm_bindgen(inline_js = "export let to_wisp_v2_handshake = x => x;")]
extern "C" {
	pub fn to_wisp_v2_handshake(val: JsValue) -> JsWispV2Handshake;
}

#[wasm_bindgen]
pub struct ProtocolExtensionBuilders(Vec<AnyProtocolExtensionBuilder>);
#[wasm_bindgen]
impl ProtocolExtensionBuilders {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Self {
		Self(vec![])
	}

	pub fn js(&mut self, builder: JsProtocolExtensionBuilder) {
		self.0.push(AnyProtocolExtensionBuilder::new(builder));
	}
}

pub fn extension_to_jsval(ext: &mut AnyProtocolExtension) -> JsValue {
	if let Some(js) = ext.downcast_mut::<JsProtocolExtension>() {
		js.js_host.clone()
	} else {
		unreachable!()
	}
}

#[wasm_bindgen]
pub struct JsWispV2Handshake(#[wasm_bindgen(skip)] pub WispV2Handshake);

#[wasm_bindgen]
impl JsWispV2Handshake {
	#[wasm_bindgen(constructor)]
	pub fn new(builders: ProtocolExtensionBuilders, middleware: Function) -> Self {
		let closure: Box<wisp_mux::WispV2Middleware> =
			Box::new(move |extensions: &mut Vec<AnyProtocolExtensionBuilder>| {
				let middleware = middleware.clone();
				Box::pin(async move {
					let array = Array::new();
					for extension in extensions {
						if let Some(js) = extension.downcast_mut::<JsProtocolExtensionBuilder>() {
							array.push(&js.js_host);
						} else {
							unreachable!()
						}
					}

					let output = middleware
						.call1(&JsValue::NULL, &array.clone().into())
						.map_err(|x| {
							WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x)))
						})?;

					SendWrapper(JsFuture::from(output.unchecked_into::<Promise>()))
						.await
						.map_err(|x| {
							WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x)))
						})?;

					Ok(())
				})
			});

		Self(WispV2Handshake::new_with_middleware(builders.0, closure))
	}
}
