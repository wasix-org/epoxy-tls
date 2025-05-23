#![feature(impl_trait_in_assoc_type)]

use std::marker::PhantomData;

use futures_rustls::rustls;
use wasm_bindgen::{JsError, JsValue};
use wisp_mux::WispError;

mod client;
mod provider;

mod log;
mod send_wrapper;
mod sink_map;

#[derive(thiserror::Error, Debug)]
pub enum EpoxyError {
	#[error("Wisp: {0}")]
	Wisp(#[from] WispError),
	#[error("IO: {0}")]
	IO(#[from] std::io::Error),
	#[error("TLS: {0}")]
	Rustls(#[from] rustls::Error),
	#[error("HTTP: {0}")]
	HyperClient(#[from] hyper_util_wasm::client::legacy::Error),
	#[error("HTTP: {0}")]
	Http(#[from] hyper::http::Error),

	#[error("Invalid DNS name: {0}")]
	InvalidDnsName(String),
	#[error("Invalid URL scheme: {0:?}")]
	InvalidUrlScheme(Option<String>),

	#[error("No URL host")]
	NoUrlHost,
	#[error("No URL port")]
	NoUrlPort,

	#[error("JS: invalid value")]
	InvalidJsValue,

	#[error("JsError({0})")]
	JsError(String),
}

impl From<EpoxyError> for JsValue {
	fn from(value: EpoxyError) -> Self {
		JsError::from(value).into()
	}
}

#[link(wasm_import_module = "__wbindgen_placeholder__")]
unsafe extern "C" {
	fn __wbindgen_debug_string(ret: *mut [usize; 2], idx: u32) -> ();
}

struct JsValueInner {
	idx: u32,
	_marker: PhantomData<*mut u8>,
}
impl JsValueInner {
	pub fn as_debug_string(&self) -> String {
		unsafe {
			let mut ret = [0; 2];
			__wbindgen_debug_string(&raw mut ret, self.idx);
			let data = Vec::from_raw_parts(ret[0] as *mut u8, ret[1], ret[1]);
			String::from_utf8_unchecked(data)
		}
	}
}

trait EpoxyJsErrorExt<T> {
	fn js_error(self) -> Result<T, EpoxyError>;
}
impl<T, E: Into<JsValue>> EpoxyJsErrorExt<T> for Result<T, E> {
	fn js_error(self) -> Result<T, EpoxyError> {
		self.map_err(|x| {
			let inner = unsafe { std::mem::transmute::<JsValue, JsValueInner>(x.into()) };
			EpoxyError::JsError(inner.as_debug_string())
		})
	}
}

trait EpoxyErrorExt<T> {
	fn js_invalid(self) -> Result<T, EpoxyError>;
	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError>;
}
impl<T, E: std::fmt::Debug> EpoxyErrorExt<T> for Result<T, E> {
	fn js_invalid(self) -> Result<T, EpoxyError> {
		self.map_err(|_| EpoxyError::InvalidJsValue)
	}

	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError> {
		self.map_err(|_| EpoxyError::InvalidDnsName(name))
	}
}
