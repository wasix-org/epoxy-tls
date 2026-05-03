import {
	EitherSocketProvider as EpxEitherSocketProvider,
	JsProvider as EpxJsProvider,
	WispProvider as EpxWispProvider,
	JsWispV2Handshake,
	ProtocolExtensionBuilders,
	WasmProvider,
	WasmWispProvider,
} from "epoxy/wbg";
/* EXTENDED.START */
import {
	TorSocketProvider as EpxTorSocketProvider,
	TorStateMgrCallbacks,
} from "epoxy/wbg";
/* EXTENDED.END */
import { WebSocketStream } from "./websocketstream";
import { ProtocolExtensionBuilder, WispExtensions } from "./wispExtension";

export type ProviderResult = [
	readable: ReadableStream<Uint8Array<ArrayBuffer>>,
	writable: WritableStream<Uint8Array<ArrayBuffer>>,
];

abstract class Provider<T, P> {
	// @internal
	_provider: T;
	// @internal
	_map: (provider: T) => P;
	// @internal
	used: boolean = false;

	constructor(provider: T, map: (provider: T) => P) {
		this._provider = provider;
		this._map = map;
	}

	// @internal
	clone(): T | undefined {
		return undefined;
	}

	// @internal
	get provider(): P {
		let cloned = this.clone();
		if (!cloned) {
			if (this.used) throw new Error("Provider already used");
			this.used = true;
			cloned = this._provider;
		}

		return this._map(cloned);
	}
}

export class JsProvider extends Provider<EpxJsProvider, WasmWispProvider> {
	constructor(
		func: (host: string) => Promise<ProviderResult> | ProviderResult
	) {
		super(new EpxJsProvider(async (host) => await func(host)), (x) =>
			x.box_wisp()
		);
	}
}

export class WebSocketJsProvider extends JsProvider {
	constructor() {
		super(async (host) => {
			let stream = new WebSocketStream<Uint8Array<ArrayBuffer>>(host);
			let { readable, writable } = await stream.opened;

			return [readable, writable];
		});
	}
}

export class JsSocketProvider extends Provider<EpxJsProvider, WasmProvider> {
	constructor(
		func: (
			host: string,
			port: number
		) => Promise<ProviderResult> | ProviderResult
	) {
		super(
			new EpxJsProvider(async (host, port) => await func(host, port)),
			(x) => x.box()
		);
	}
}

export class WsProxyJsSocketProvider extends JsSocketProvider {
	constructor(wsproxy: string) {
		if (!["ws:", "wss:"].includes(new URL(wsproxy).protocol))
			throw new Error("Invalid wsproxy base url. Needs ws or wss protocol");
		super(async (host, port) => {
			let url = new URL(wsproxy);

			if (!url.pathname.endsWith("/")) url.pathname += "/";

			url.pathname += `${encodeURIComponent(host)}:${port}`;

			let stream = new WebSocketStream<Uint8Array<ArrayBuffer>>(url.toString());
			let { readable, writable } = await stream.opened;

			return [readable, writable];
		});
	}
}

export interface WispV2Handshake {
	builders: ProtocolExtensionBuilder[];
}

export class WispSocketProvider extends Provider<
	EpxWispProvider,
	WasmProvider
> {
	clone() {
		return this._provider.dup();
	}

	constructor(
		provider: JsProvider,
		server: string,
		connectionPrefs?: () => [
			v2: WispV2Handshake | undefined,
			requiredExts: number[],
		]
	) {
		let v2;
		if (connectionPrefs) {
			v2 = () => {
				let [v2, required] = connectionPrefs();

				let handshake: JsWispV2Handshake | undefined;
				if (v2) {
					let builders = new ProtocolExtensionBuilders();
					for (let builder of v2.builders) {
						builder.appendTo(builders);
					}
					handshake = new JsWispV2Handshake(builders, async () => {});
				}

				return [handshake, new Uint8Array(required)];
			};
		}
		super(EpxWispProvider.new(provider.provider, server, v2), (x) => x.box());
	}

	async replaceMux() {
		await this._provider.replace_mux();
	}

	async getExtensions(): Promise<WispExtensions | undefined> {
		let ret = await this._provider.get_extensions();
		if (ret) {
			return new WispExtensions(ret);
		}
	}
}

export class EitherSocketProvider extends Provider<
	EpxEitherSocketProvider,
	WasmProvider
> {
	constructor(
		selector: (host: string, port: number) => "left" | "right",
		left: SocketProvider,
		right: SocketProvider
	) {
		super(
			new EpxEitherSocketProvider(selector, left.provider, right.provider),
			(x) => x.box()
		);
	}
}

/* EXTENDED.START */
export type TorStorage = TorStateMgrCallbacks;

export class TorSocketProvider extends Provider<
	EpxTorSocketProvider,
	WasmProvider
> {
	constructor(
		backend: Exclude<SocketProvider, TorSocketProvider> | "snowflake",
		storage: TorStorage
	) {
		const inner = backend === "snowflake" ? undefined : backend.provider;
		super(new EpxTorSocketProvider(inner, storage), (x) => x.box());
	}

	async bootstrap() {
		await this._provider.bootstrap();
	}
}
/* EXTENDED.END */

export type SocketProvider =
	| JsSocketProvider
	| WispSocketProvider
	| EitherSocketProvider
	/* EXTENDED.START */
	| TorSocketProvider
	/* EXTENDED.END */;
