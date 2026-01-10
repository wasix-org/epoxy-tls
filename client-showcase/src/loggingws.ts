// https://github.com/CarterLi/websocketstream-polyfill modified for logging
import { createState } from "dreamland/core";
export type Packet = {
	type: "rx" | "tx",
	text: string,
	hex: string,
}

export const logger = createState({ logged: [] as Packet[]});

function split<T>(arr: T[], len: number): T[][] {
	const ret = [];
	let i;
	for (i = 0; i < arr.length; i += len) {
		ret.push(arr.slice(i, i + len));
	}
	if (i != arr.length)
		ret.push(arr.slice(i));
	return ret;
}

function toHex(arr: ArrayBuffer): string {
	return split([...new Uint8Array(arr)]
		.map(x => x.toString(16).padStart(2, '0')), 2)
		.map(x => x.join(''))
		.join(' ');
}

function toText(arr: ArrayBuffer): string {
	return [...new Uint8Array(arr)].map(x => String.fromCharCode(x)).join('');
}

export interface WebSocketConnection<T extends Uint8Array | string = Uint8Array | string> {
	readable: ReadableStream<T>;
	writable: WritableStream<T>;
	protocol: string;
	extensions: string;
}

export interface WebSocketCloseInfo {
	closeCode?: number;
	reason?: string;
}

export interface WebSocketStreamOptions {
	protocols?: string[];
	signal?: AbortSignal;
}

/**
 * [WebSocket](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket) with [Streams API](https://developer.mozilla.org/en-US/docs/Web/API/Streams_API)
 *
 * @see https://web.dev/websocketstream/
 */
export class WebSocketStream<T extends Uint8Array | string = Uint8Array | string> {
	readonly url: string;

	readonly opened: Promise<WebSocketConnection<T>>;

	readonly closed: Promise<WebSocketCloseInfo>;

	readonly close: (closeInfo?: WebSocketCloseInfo) => void;

	constructor(url: string, options: WebSocketStreamOptions = {}) {
		if (options.signal?.aborted) {
			throw new DOMException('This operation was aborted', 'AbortError');
		}

		this.url = url;

		const ws = new WebSocket(url, options.protocols ?? []);
		ws.binaryType = "arraybuffer";
		logger.logged = [];

		const closeWithInfo = ({ closeCode: code, reason }: WebSocketCloseInfo = {}) => ws.close(code, reason);

		this.opened = new Promise((resolve, reject) => {
			ws.onopen = () => {
				resolve({
					readable: new ReadableStream<T>({
						start(controller) {
							ws.onmessage = ({ data }) => {
								logger.logged = [...logger.logged, { type: "rx", text: toText(data), hex: toHex(data) }];
								controller.enqueue(data);
							}
							ws.onerror = e => controller.error(e);
						},
						cancel: closeWithInfo,
					}),
					writable: new WritableStream<T>({
						write(chunk) {
							const data = chunk as unknown as ArrayBuffer;
							logger.logged = [...logger.logged, { type: "tx", text: toText(data), hex: toHex(data) }];
							ws.send(data);
						},
						abort() { ws.close(); },
						close: closeWithInfo,
					}),
					protocol: ws.protocol,
					extensions: ws.extensions,
				});
				ws.removeEventListener('error', reject);
			};
			ws.addEventListener('error', reject);
		});

		this.closed = new Promise<WebSocketCloseInfo>((resolve, reject) => {
			ws.onclose = ({ code, reason }) => {
				resolve({ closeCode: code, reason });
				ws.removeEventListener('error', reject);
			};
			ws.addEventListener('error', reject);
		});

		if (options.signal) {
			options.signal.onabort = () => ws.close();
		}

		this.close = closeWithInfo;
	}
}
