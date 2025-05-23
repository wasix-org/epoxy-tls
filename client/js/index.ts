// @ts-expect-error
import wbgInit, { Client } from "epoxy/wbg";
// @ts-expect-error
import wasm from "epoxy/wasm";

export class EpoxyClient {
	// @internal
	client: Client;

	constructor() {
		this.client = new Client(async (host: string, port: string) => {
			throw "hi";
		});
	}

	async fetch() {
		await this.client.request("https://google.com");
	}
}

export async function init() {
	await wbgInit({ module_or_path: await wasm() });
}
