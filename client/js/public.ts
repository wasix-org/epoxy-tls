import epxVersion from "epoxy/version";

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
export type { EpoxyResponse } from "./client";
export { EpoxyWS } from "./websocket";
export type {
	EpoxyRawHeaders,
	EpoxyWSChunk,
	EpoxyWSCloseInfo,
	EpoxyWebSocketOptions,
} from "./websocket";
export let version = { package: epxVersion.version, git: epxVersion.git };
