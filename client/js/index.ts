// @ts-expect-error
import { Client, init } from "epoxy/wbg";

export class EpoxyClient {
	// @internal
	client: Client;

	constructor() {
		this.client = new Client(async (host: string, port: string) => {
			throw "hi";
		});
	}

	async fetch() {
		await this.client.fetch("https://google.com");
	}
}
