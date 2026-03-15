import {
	JsProtocolExtensionBuilder as EpxJsExtBuilder,
	JsProtocolExtension as EpxJsExt,
	JsTransportRead,
	JsTransportWrite,
	ProtocolExtensionBuilders,
} from "epoxy/wbg";

export type Role = "client" | "server";

export class TransportRead {
	// @internal
	inner: JsTransportRead;

	// @internal
	constructor(inner: JsTransportRead) {
		this.inner = inner;
	}
}

export class TransportWrite {
	// @internal
	inner: JsTransportWrite;

	// @internal
	constructor(inner: JsTransportWrite) {
		this.inner = inner;
	}
}

export abstract class ProtocolExtension {
	abstract readonly id: number;
}

export abstract class ProtocolExtensionBuilder {
	abstract readonly id: number;
	// @internal
	abstract inner: unknown;
	// @internal
	abstract appendTo(builders: ProtocolExtensionBuilders): void;
}

export abstract class JsProtocolExtension extends ProtocolExtension {
	// @internal
	inner: EpxJsExt;

	readonly id: number;

	constructor(id: number, packets: number[], congestionStreams: number[]) {
		super();
		this.id = id;

		let encode = () => {
			try {
				return this.encode();
			} catch (err) {
				console.warn(
					"[epoxy] encode failed! this will crash the wasm module",
					err
				);
				throw err;
			}
		};
		let handleHandshake = async (read, write) => {
			await this.handleHandshake(
				new TransportRead(read),
				new TransportWrite(write)
			);
		};
		let handlePacket = async (type, packet, read, write) => {
			await this.handlePacket(
				type,
				packet,
				new TransportRead(read),
				new TransportWrite(write)
			);
		};

		this.inner = new EpxJsExt(
			id,
			new Uint8Array(packets),
			new Uint8Array(congestionStreams),
			encode,
			handleHandshake,
			handlePacket,
			this
		);
	}

	abstract encode(): Uint8Array;
	handleHandshake(
		read: TransportRead,
		write: TransportWrite
	): Promise<void> | void {}
	handlePacket(
		type: number,
		packet: Uint8Array,
		read: TransportRead,
		write: TransportWrite
	): Promise<void> | void {}
}

export abstract class JsProtocolExtensionBuilder extends ProtocolExtensionBuilder {
	// @internal
	inner: EpxJsExtBuilder;

	readonly id: number;

	constructor(id: number) {
		super();
		this.id = id;

		let buildFromBytes = (bytes, role) => {
			let ext = this.buildFromBytes(bytes, role);
			if (ext.id !== this.id) throw new Error("incorrect id");
			return ext.inner;
		};
		let buildToExtension = (role) => {
			let ext = this.buildToExtension(role);
			if (ext.id !== this.id) throw new Error("incorrect id");
			return ext.inner;
		};

		this.inner = new EpxJsExtBuilder(
			id,
			buildFromBytes,
			buildToExtension,
			this
		);
	}

	abstract buildFromBytes(bytes: Uint8Array, role: Role): JsProtocolExtension;
	abstract buildToExtension(role: Role): JsProtocolExtension;

	appendTo(builders: ProtocolExtensionBuilders) {
		builders.js(this.inner);
	}
}
