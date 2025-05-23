use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
unsafe extern "C" {
	#[wasm_bindgen(js_namespace = console, js_name = log)]
	pub fn __console_log(s: &str);
}

#[macro_export]
macro_rules! console_log {
    ($($t:tt)*) => ($crate::log::__console_log(&format_args!($($t)*).to_string()))
}
