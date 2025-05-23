#![feature(impl_trait_in_assoc_type)]

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

	#[error("{0}")]
	JsError(String),
}

impl From<EpoxyError> for JsValue {
	fn from(value: EpoxyError) -> Self {
		JsError::from(value).into()
	}
}

trait EpoxyErrorExt<T> {
	fn js_invalid(self) -> Result<T, EpoxyError>;
	fn js_error(self) -> Result<T, EpoxyError>;
	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError>;
}
impl<T, E: std::fmt::Debug> EpoxyErrorExt<T> for Result<T, E> {
	fn js_invalid(self) -> Result<T, EpoxyError> {
		self.map_err(|_| EpoxyError::InvalidJsValue)
	}

	fn js_error(self) -> Result<T, EpoxyError> {
		self.map_err(|x| EpoxyError::JsError(format!("{x:?}")))
	}

	fn invalid_dns_name(self, name: String) -> Result<T, EpoxyError> {
		self.map_err(|_| EpoxyError::InvalidDnsName(name))
	}
}
