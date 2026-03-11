import {
	JsProvider as EpxJsProvider,
	WispProvider as EpxWispProvider,
	WasmProvider,
	WasmWispProvider,
} from "epoxy/wbg";
import { WebSocketStream } from "./websocketstream";

export type ProviderResult = [
	readable: ReadableStream<Uint8Array<ArrayBuffer>>,
	writable: WritableStream<Uint8Array<ArrayBuffer>>,
];

export class JsProvider {
	func: (host: string) => Promise<ProviderResult> | ProviderResult;

	constructor(func: typeof this.func) {
		this.func = func;
	}

	// @internal
	into(): WasmWispProvider {
		return EpxJsProvider.provider_wisp(async (host) => await this.func(host));
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

export class JsSocketProvider {
	func: (
		host: string,
		port: number
	) => Promise<ProviderResult> | ProviderResult;

	constructor(func: typeof this.func) {
		this.func = func;
	}

	// @internal
	into(): WasmProvider {
		return EpxJsProvider.provider(
			async (host, port) => await this.func(host, port)
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

export class WispSocketProvider {
	provider: JsProvider;
	server: string;

	constructor(provider: typeof this.provider, server: string) {
		this.provider = provider;
		this.server = server;
	}

	// @internal
	into(): WasmProvider {
		return EpxWispProvider.new(this.provider.into(), this.server);
	}
}

export type SocketProvider = JsSocketProvider | WispSocketProvider;
