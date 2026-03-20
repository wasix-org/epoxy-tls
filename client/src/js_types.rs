use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(typescript_custom_section)]
const JS_TYPES_TS: &str = r#"
export type RawHeader = [name: string, value: Uint8Array];
export type RawHeaders = RawHeader[];

export type ProviderResult = [ReadableStream<Uint8Array>, WritableStream<Uint8Array>];
export type MaybePromise<T> = T | Promise<T>;
export type JsProviderCallback = (...args: [host: string] | [host: string, port: number]) => MaybePromise<ProviderResult>;

export type WispRole = "client" | "server";
export type JsProtocolExtensionBuilderFromBytes = (bytes: Uint8Array, role: WispRole) => JsProtocolExtension;
export type JsProtocolExtensionBuilderToExtension = (role: WispRole) => JsProtocolExtension;
export type JsProtocolExtensionEncode = () => Uint8Array;
export type JsProtocolExtensionHandshake = (read: JsTransportRead, write: JsTransportWrite) => MaybePromise<void>;
export type JsProtocolExtensionPacket = (type: number, packet: Uint8Array, read: JsTransportRead, write: JsTransportWrite) => MaybePromise<void>;

export type JsWispV2ConnectionPrefs = () => [JsWispV2Handshake | undefined, Uint8Array];
export type JsWispV2Middleware = (extensions: unknown[]) => MaybePromise<void>;
export type WispExtensionList = unknown[];
"#;

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(typescript_type = "RawHeaders")]
	pub type RawHeaders;

	#[wasm_bindgen(typescript_type = "JsProviderCallback")]
	pub type JsProviderCallback;

	#[wasm_bindgen(typescript_type = "JsProtocolExtensionBuilderFromBytes")]
	pub type JsProtocolExtensionBuilderFromBytes;

	#[wasm_bindgen(typescript_type = "JsProtocolExtensionBuilderToExtension")]
	pub type JsProtocolExtensionBuilderToExtension;

	#[wasm_bindgen(typescript_type = "JsProtocolExtensionEncode")]
	pub type JsProtocolExtensionEncode;

	#[wasm_bindgen(typescript_type = "JsProtocolExtensionHandshake")]
	pub type JsProtocolExtensionHandshake;

	#[wasm_bindgen(typescript_type = "JsProtocolExtensionPacket")]
	pub type JsProtocolExtensionPacket;

	#[wasm_bindgen(typescript_type = "JsWispV2ConnectionPrefs")]
	pub type JsWispV2ConnectionPrefs;

	#[wasm_bindgen(typescript_type = "JsWispV2Middleware")]
	pub type JsWispV2Middleware;

	#[wasm_bindgen(typescript_type = "WispExtensionList")]
	pub type WispExtensionList;
}
