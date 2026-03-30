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
export type { EpoxyResponse, EpoxyRawHeaders } from "./util";

/* FULL.START */
export { EpoxyWS } from "./websocket";
export type {
	EpoxyWSChunk,
	EpoxyWSCloseInfo,
	EpoxyWebSocketOptions,
} from "./websocket";
/* FULL.END */

export let version = { package: epxVersion.version, git: epxVersion.git };
