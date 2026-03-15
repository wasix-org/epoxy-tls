import { WispProtocolExtensions } from "epoxy/wbg";
import { ProtocolExtension } from "./js";
import { wrapBuiltinExtension } from "./builtin";

/// Holding this object will block the wisp provider from doing anything. Don't hold extensions past drop()
export class WispExtensions {
	// @internal
	inner: WispProtocolExtensions;
	// @internal
	arr: any[];

	// @internal
	constructor(inner: WispProtocolExtensions) {
		this.inner = inner;
		this.arr = inner.arr();
	}

	get(idx: number): ProtocolExtension | undefined {
		let ext = this.arr[idx];
		if (!ext) return;

		return wrapBuiltinExtension(ext) ?? (ext as ProtocolExtension);
	}

	get length(): number {
		return this.arr.length;
	}

	drop() {
		this.arr = [];
		this.inner.free();
	}
}

export {
	Role,
	TransportRead,
	TransportWrite,
	ProtocolExtension,
	ProtocolExtensionBuilder,
	JsProtocolExtension,
	JsProtocolExtensionBuilder,
} from "./js";
export {
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
} from "./builtin";
