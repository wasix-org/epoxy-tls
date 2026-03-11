// @ts-expect-error
import wbgInit, { Client, JsProvider, WispProvider } from "epoxy/wbg";
// @ts-expect-error
import wasm from "epoxy/wasm";
import { WebSocketStream } from "./websocketstream";

export class EpoxyClient {
	// @internal
	client: Client;

	constructor() {
		let backend = JsProvider.provider_wisp(async (server: string) => {
			const stream = new WebSocketStream(server);
			const { readable, writable } = await stream.opened;

			return [readable, writable];
		});
		let wisp = WispProvider.new(backend, "wss://anura.pro/");

		this.client = new Client(wisp);
	}

	async fetch() {
		let abort = new AbortController();
		this.client.request("https://google.com", abort.signal);
	}
}

export async function init() {
	await wbgInit({ module_or_path: await wasm() });
}
