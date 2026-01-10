import epoxyInit, { EpoxyClient, EpoxyClientOptions, info as epoxyInfo } from "@mercuryworkshop/epoxy-tls/minimal-epoxy";
import settings from "./store";
import { WebSocketStream } from "./loggingws";

export let epoxyVersion = epoxyInfo.version + epoxyInfo.commit + epoxyInfo.release;

const EPOXY_PATH = "/epoxy/epoxy.wasm";

let cache: Cache = await window.caches.open("epoxy");
let initted: boolean = false;

let currentClient: EpoxyClient;
let currentWispUrl: string;

async function evictEpoxy() {
	await cache.delete(EPOXY_PATH);
}

async function instantiateEpoxy() {
	if (!await cache.match(EPOXY_PATH)) {
		await cache.add(EPOXY_PATH);
	}
	const module = await cache.match(EPOXY_PATH);
	await epoxyInit({ module_or_path: module });
	initted = true;
}

export async function createEpoxy() {
	let options = new EpoxyClientOptions();
	options.user_agent = navigator.userAgent;
	options.udp_extension_required = false;

	currentWispUrl = settings.wispServer;
	//@ts-ignore
	currentClient = new EpoxyClient(async () => {
		try {
			const wss = new WebSocketStream(settings.wispServer);
			const ws = await wss.opened;
			return { read: ws.readable, write: ws.writable };
		} catch {
			throw new Error("Failed to connect to Wisp Server: " + settings.wispServer);
		}
	}, options);
}

export async function fetch(url: string, options?: any): Promise<Response> {
	if (!initted) {
		if (epoxyVersion === settings.epoxyVersion) {
			await instantiateEpoxy();
		} else {
			await evictEpoxy();
			await instantiateEpoxy();
			console.log(`evicted epoxy "${settings.epoxyVersion}" from cache because epoxy "${epoxyVersion}" is available`);
			settings.epoxyVersion = epoxyVersion;
		}
	}

	if (currentWispUrl !== settings.wispServer) {
		await createEpoxy();
	}
	try {
		return await currentClient.fetch(url, options);
	} catch (err2) {
		let err = err2 as Error;
		console.log(err);

		throw err;
	}
}

// @ts-ignore
window.epoxyFetch = fetch;
