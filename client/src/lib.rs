#![feature(impl_trait_in_assoc_type)]

use core::convert::Into;

use futures_rustls::rustls;
use wasm_bindgen::{JsError, JsValue};
use wisp_mux::WispError;

mod client;
mod provider;

mod log;
mod send_wrapper;
mod sink_map;

#[cfg(feature = "debug")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
fn main() {
	std::panic::set_hook(Box::new(|info| {
		console_log!("{}", info);
	}));
}

#[derive(thiserror::Error, Debug)]
pub enum EpoxyError {
	#[error("Wisp: {0}")]
	Wisp(#[from] WispError),
	#[error("IO: {0}")]
	IO(#[from] std::io::Error),
	#[error("TLS: {0}")]
	Rustls(#[from] rustls::Error),
	#[error("Hyper client: {0}")]
	HyperClient(#[from] hyper_util_wasm::client::legacy::Error),
	#[error("Hyper: {0}")]
	Hyper(#[from] hyper::Error),
	#[error("HTTP: {0}")]
	Http(#[from] hyper::http::Error),

	#[error("{0}")]
	InvalidMethod(#[from] http::method::InvalidMethod),
	#[error("{0}")]
	InvalidUri(#[from] http::uri::InvalidUri),
	#[error("{0}")]
	InvalidHeaderName(#[from] http::header::InvalidHeaderName),
	#[error("{0}")]
	InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
	#[error("Invalid DNS name: {0}")]
	InvalidDnsName(String),
	#[error("Invalid URL scheme: {0:?}")]
	InvalidUrlScheme(Option<String>),

	#[error("No URL host")]
	NoUrlHost,
	#[error("No URL port")]
	NoUrlPort,

	#[error("Aborted")]
	Aborted,

	#[error("JS: invalid value: {0}")]
	InvalidJsValue(String),
	#[error("JsError({0})")]
	JsError(String),
}
impl EpoxyError {
	pub fn js_error(val: impl Into<JsValue>) -> Self {
		EpoxyError::JsError(jsval_debug(val))
	}
}

impl From<EpoxyError> for JsValue {
	fn from(value: EpoxyError) -> Self {
		JsError::from(value).into()
	}
}

#[wasm_bindgen::prelude::wasm_bindgen(wasm_bindgen = wasm_bindgen, raw_module = "__wbindgen_placeholder__")]
extern "C" {
	fn __wbindgen_debug_string(js: &JsValue) -> String;
}

pub fn jsval_debug(val: impl Into<JsValue>) -> String {
	__wbindgen_debug_string(&val.into())
}

trait EpoxyJsValErrorExt<T> {
	fn js_invalid(self) -> Result<T, EpoxyError>;
	fn js_error(self) -> Result<T, EpoxyError>;
}
impl<T, E: Into<JsValue>> EpoxyJsValErrorExt<T> for Result<T, E> {
	fn js_error(self) -> Result<T, EpoxyError> {
		self.map_err(|x| EpoxyError::js_error(x))
	}

	fn js_invalid(self) -> Result<T, EpoxyError> {
		self.map_err(|x| EpoxyError::InvalidJsValue(jsval_debug(x)))
	}
}

trait EpoxyErrorExt<T> {
	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError>;
}
impl<T, E: std::fmt::Debug> EpoxyErrorExt<T> for Result<T, E> {
	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError> {
		self.map_err(|_| EpoxyError::InvalidDnsName(name))
	}
}
