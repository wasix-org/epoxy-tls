use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(typescript_custom_section)]
const TOR_JS_TYPES_TS: &str = r#"
export type TorStateMgrCallbacks = {
	load: (key: string) => string | null | undefined;
	store: (key: string, value: string) => void;
};

export type TorBackendSelector = (() => "snowflake") | unknown;
"#;

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(typescript_type = "TorStateMgrCallbacks")]
	pub type JsStateMgrCallbacks;
}
