import wbgInit from "epoxy/wbg";
import wasm from "epoxy/wasm";

export async function init() {
	await wbgInit({ module_or_path: await wasm() });
}

export {
	JsProvider,
	JsSocketProvider,
	WebSocketJsProvider,
	WsProxyJsSocketProvider,
	WispSocketProvider,
	ProviderResult,
} from "./provider";
export {
	JsProtocolExtensionBuilder,
	JsProtocolExtension,
} from "./wispExtension";
export { EpoxyClient } from "./client";
