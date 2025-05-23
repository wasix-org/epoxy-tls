// @ts-expect-error
import wbgInit, { Client } from "epoxy/wbg";
// @ts-expect-error
import wasm from "epoxy/wasm";
import { WebSocketStream } from "./websocketstream";

export class EpoxyClient {
	// @internal
	client: Client;

	constructor() {
		this.client = new Client(async (host: string, port: number) => {
			const stream = new WebSocketStream(`wss://anura.pro/${host}:${port}`);
			const { readable, writable } = await stream.opened;

			let transform = new TransformStream({
				transform(val, controller) {
					controller.enqueue(new Uint8Array(val));
				}
			});

			return [readable.pipeThrough(transform), writable];
		});
	}

	async fetch() {
		await this.client.request("https://google.com");
	}
}

export async function init() {
	await wbgInit({ module_or_path: await wasm() });
}
