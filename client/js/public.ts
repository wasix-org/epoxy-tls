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
export let version = { package: epxVersion.version, git: epxVersion.git };
