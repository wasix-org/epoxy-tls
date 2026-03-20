import { Client, ClientReqBuilder, JsSocket, Redirect } from "epoxy/wbg";
import { SocketProvider } from "./provider";
import { decode, encode } from "./util";

interface NormalizedRequest {
	uri: string;
	method: string;
	headers: [string, string][];
	redirect: Redirect;
	body?: ReadableStream;
	length?: bigint;
	signal: AbortSignal;
}

function normalizeRedirect(redirect: RequestRedirect): Redirect {
	switch (redirect) {
		case "follow":
			return Redirect.Follow;
		case "manual":
			return Redirect.Manual;
		case "error":
			return Redirect.Error;
	}
}

const HTTP_TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

function normalizeMethod(method: string): string {
	if (!HTTP_TOKEN.test(method)) {
		throw new TypeError(`Invalid HTTP method: ${method}`);
	}

	switch (method.toLowerCase()) {
		case "delete":
			return "DELETE";
		case "get":
			return "GET";
		case "head":
			return "HEAD";
		case "options":
			return "OPTIONS";
		case "post":
			return "POST";
		case "put":
			return "PUT";
		default:
			return method;
	}
}

function normalizeHeaderValue(name: string, value: string): string {
	let normalized = new Headers([[name, value]]).get(name);
	if (normalized === null) {
		throw new TypeError(`Invalid HTTP header: ${name}`);
	}

	return normalized;
}

function inferBodyLength(
	body: BodyInit | null | undefined
): bigint | undefined {
	if (body == null) {
		return undefined;
	}

	if (typeof body === "string") {
		return BigInt(encode(body).byteLength);
	}

	if (body instanceof Blob) {
		return BigInt(body.size);
	}

	if (body instanceof URLSearchParams) {
		return BigInt(encode(body.toString()).byteLength);
	}

	if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) {
		return BigInt(body.byteLength);
	}

	return undefined;
}

class HeaderList {
	#entries: { name: string; lowerName: string; value: string }[] = [];

	append(name: string, value: string) {
		let normalizedName = String(name);
		let normalizedValue = normalizeHeaderValue(normalizedName, String(value));
		let lowerName = normalizedName.toLowerCase();

		this.#entries.push({
			name: normalizedName,
			lowerName,
			value: normalizedValue,
		});
	}

	has(name: string): boolean {
		let lowerName = name.toLowerCase();
		return this.#entries.some((x) => x.lowerName === lowerName);
	}

	fill(init: HeadersInit) {
		if (init instanceof Headers) {
			for (let [name, value] of init) {
				this.append(name, value);
			}
			return;
		}

		if (Array.isArray(init)) {
			for (let header of init) {
				if (header.length !== 2) {
					throw new TypeError("Each header pair must be a name/value tuple");
				}

				this.append(header[0], header[1]);
			}
			return;
		}

		for (let [name, value] of Object.entries(init)) {
			this.append(name, value);
		}
	}

	toTuples(): [string, string][] {
		return this.#entries.map((x) => [x.name, x.value]);
	}
}

function normalizeRequest(
	resource: Request | URL | string,
	options: RequestInit = {}
): NormalizedRequest {
	let inputMethod = resource instanceof Request ? resource.method : "GET";
	let method = normalizeMethod(options.method ?? inputMethod);
	let headers = new HeaderList();

	if (options.headers !== undefined) {
		headers.fill(options.headers);
	} else if (resource instanceof Request) {
		headers.fill(resource.headers);
	}

	let hasInitBody = "body" in options && options.body != null;
	let inheritedBody = resource instanceof Request ? resource.body : null;
	if (
		(hasInitBody || inheritedBody !== null) &&
		(method === "GET" || method === "HEAD")
	) {
		throw new TypeError("Request with GET/HEAD method cannot have body");
	}

	let browserInit: RequestInit & { duplex?: string } = {
		...options,
		headers: undefined,
		method: hasInitBody
			? "POST"
			: resource instanceof Request
				? resource.method
				: undefined,
	};
	if (!hasInitBody) {
		delete browserInit.body;
	}

	let browserRequest = new Request(resource, browserInit);
	let body = browserRequest.body as ReadableStream<Uint8Array> | null;
	let length = inferBodyLength(hasInitBody ? options.body : undefined);
	let contentType = browserRequest.headers.get("content-type");
	let redirect = normalizeRedirect(browserRequest.redirect);
	if (contentType !== null && !headers.has("content-type")) {
		headers.append("Content-Type", contentType);
	}

	return {
		uri: browserRequest.url,
		method,
		headers: headers.toTuples(),
		redirect,
		body: body ?? undefined,
		length,
		signal: browserRequest.signal,
	};
}

let defaultUA =
	globalThis?.navigator?.userAgent ||
	"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";
export class EpoxyClient {
	// @internal
	client: Client;

	constructor(provider: SocketProvider) {
		this.client = new Client(provider.provider, defaultUA);
	}

	get userAgent(): string {
		return this.client.get_ua();
	}
	set userAgent(val: string) {
		this.client.set_ua(val);
	}

	async fetch(
		resource: Request | URL | string,
		options: RequestInit = {}
	): Promise<Response> {
		let normalized = normalizeRequest(resource, options);

		let request = new ClientReqBuilder();
		request.uri(normalized.uri);
		request.method(normalized.method);

		for (let [header, value] of normalized.headers) {
			request.header(header, value);
		}

		let ret = await this.client.request(
			request,
			normalized.signal,
			normalized.redirect,
			normalized.body,
			normalized.length
		);

		let status = ret.status();
		let statusText = ret.status_text();
		let url = ret.uri();
		let rawRawHeaders = ret.headers();
		let body = ret.body();

		let rawHeaders: Record<string, string[]> = Object.create(null);
		let headers = new Headers();
		for (let [name, rawValue] of rawRawHeaders) {
			let value = decode(rawValue);
			headers.append(name, value);

			if (rawHeaders[name] === undefined) {
				rawHeaders[name] = [];
			}
			rawHeaders[name].push(value);
		}

		let res = new Response(body, {
			status,
			statusText,
			headers,
		});
		Object.defineProperty(res, "url", { value: url });
		(res as any).rawHeaders = rawHeaders;

		return res;
	}

	async connect(
		host: string,
		port: number,
		bufferSize: number = 16384
	): Promise<TcpStream> {
		return new TcpStream(await this.client.connect(host, port, bufferSize));
	}
	async connectTls(
		host: string,
		port: number,
		bufferSize: number = 16384
	): Promise<TlsStream> {
		return new TlsStream(await this.client.connect_tls(host, port, bufferSize));
	}
}

export class TcpStream {
	read: ReadableStream<Uint8Array>;
	write: WritableStream<Uint8Array>;

	// @internal
	constructor(inner: JsSocket) {
		this.read = inner[0];
		this.write = inner[1];
	}
}

export class TlsStream {
	read: ReadableStream<Uint8Array>;
	write: WritableStream<Uint8Array>;

	// @internal
	constructor(inner: JsSocket) {
		this.read = inner[0];
		this.write = inner[1];
	}
}
