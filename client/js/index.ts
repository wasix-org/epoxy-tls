import wbgInit from "epoxy/wbg";
import wasm from "epoxy/wasm";
// @ts-ignore
import epxVersion from "epoxy/version";

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
	Role,
	TransportRead,
	TransportWrite,
	ProtocolExtension,
	ProtocolExtensionBuilder,
	JsProtocolExtensionBuilder,
	JsProtocolExtension,
	UdpProtocolExtensionBuilder,
	UdpProtocolExtensionBuilderRef,
	UdpProtocolExtension,
	MotdProtocolExtensionBuilder,
	MotdProtocolExtensionBuilderRef,
	MotdProtocolExtension,
	PasswordProtocolExtensionBuilder,
	PasswordProtocolExtensionBuilderRef,
	PasswordProtocolExtension,
	CertAuthProtocolExtensionBuilder,
	CertAuthProtocolExtensionBuilderRef,
	CertAuthProtocolExtension,
	WispExtensions,
} from "./wispExtension";
export { EpoxyClient } from "./client";
export let version = { package: epxVersion.version, git: epxVersion.git };
