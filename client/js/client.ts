import { Client, ClientReqBuilder } from "epoxy/wbg";
import { SocketProvider } from "./provider";
import { decode } from "./util";

interface NormalizedRequest {
	uri: string;
	method: string;
	headers: Map<string, string[]>;
	body?: ReadableStream;
	contentType?: string;
}
function normalizeRequest(
	resource: Request | URL | string,
	options: RequestInit = {}
): NormalizedRequest {
	let uri: string;
	let method = options.method ?? "GET";

	if (resource instanceof Request) {
		uri = resource.url;
		method = options.method ?? resource.method;
	} else if (resource instanceof URL) {
		uri = resource.href;
	} else {
		uri = resource;
	}

	let headers = new Map<string, string[]>();

	function ingest(h?: HeadersInit) {
		if (!h) return;

		function append(k: string, v: string) {
			let key = k.toLowerCase();
			let list = headers.get(key);
			if (list) list.push(v);
			else headers.set(key, [v]);
		}

		if (h instanceof Headers) {
			for (let [k, v] of h) append(k, v);
		} else if (Array.isArray(h)) {
			for (let [k, v] of h) append(k, v);
		} else {
			for (let k in h) append(k, h[k]!);
		}
	}

	if (options.headers !== undefined) {
		ingest(options.headers);
	} else if (resource instanceof Request) {
		ingest(resource.headers);
	}

	let body: ReadableStream<Uint8Array> | undefined;
	let contentType: string | undefined;

	if (options.body !== undefined) {
		// create temporary request to use spec body extraction
		let tmp = new Request("http://localhost/", {
			method: "POST",
			body: options.body,
		});

		body = tmp.body as ReadableStream<Uint8Array> | null;

		let detected = tmp.headers.get("content-type");
		if (detected) contentType = detected;
	} else if (resource instanceof Request && resource.body) {
		body = resource.body as ReadableStream<Uint8Array>;
	}

	return {
		uri,
		method,
		headers,
		body,
		contentType,
	};
}

export class EpoxyClient {
	// @internal
	client: Client;

	constructor(provider: SocketProvider) {
		this.client = new Client(provider.into());
	}

	async fetch(resource: Request | URL | string, options: RequestInit): Promise<Response> {
		let normalized = normalizeRequest(resource, options);

		let request = new ClientReqBuilder();
		request.uri(normalized.uri);
		request.method(normalized.method);

		if (normalized.contentType) {
			request.header("content-type", normalized.contentType);
		}

		for (let [header, vals] of normalized.headers.entries()) {
			for (let val of vals) {
				request.header(header, val);
			}
		}

		let abort = new AbortController();
		let ret = await this.client.request(request, abort.signal, normalized.body);

		let [code, codeDesc] = ret.status();
		let rawRawHeaders = ret.headers();
		let body = ret.body();

		let rawHeaders = rawRawHeaders.map(x => [x.name(), x.values().map((x) => decode(x))] as const);
		let headers = new Headers();
		for (let [k, vs] of rawHeaders) {
			for (let v of vs) {
				headers.append(k, v);
			}
		}

		let res = new Response(body, { status: code, statusText: codeDesc, headers, });
		(res as any).rawHeaders = rawHeaders;

		return res;
	}
}
