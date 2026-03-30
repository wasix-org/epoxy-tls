import { WsWriteEvent, type WsReadEvent } from "epoxy/wbg";
import { EpoxyRawHeaders } from "./util";

export type EpoxyWSChunk = string | Uint8Array<ArrayBufferLike>;

export interface EpoxyWSCloseInfo {
	closeCode?: number;
	reason?: string;
}

export interface EpoxyWebSocketOptions {
	protocols?: string | string[];
	headers?: HeadersInit;
}

export class EpoxyWS {
	readonly readable: ReadableStream<EpoxyWSChunk>;
	readonly writable: WritableStream<EpoxyWSChunk>;
	readonly protocol: string;
	readonly headers: Headers;
	readonly rawHeaders: EpoxyRawHeaders;
	readonly closed: Promise<EpoxyWSCloseInfo>;

	#resolveClosed!: (info: EpoxyWSCloseInfo) => void;
	#rejectClosed!: (error: unknown) => void;
	#closedSettled = false;

	#writer: WritableStreamDefaultWriter<WsWriteEvent>;
	#writeQueue: Promise<void> = Promise.resolve();
	#closeSent = false;

	constructor(
		readable: ReadableStream<WsReadEvent>,
		writable: WritableStream<WsWriteEvent>,
		protocol: string,
		headers: Headers,
		rawHeaders: EpoxyRawHeaders
	) {
		this.protocol = protocol;
		this.headers = headers;
		this.rawHeaders = rawHeaders;

		this.#writer = writable.getWriter();
		this.closed = new Promise<EpoxyWSCloseInfo>((resolve, reject) => {
			this.#resolveClosed = resolve;
			this.#rejectClosed = reject;
		});

		this.readable = new ReadableStream<EpoxyWSChunk>({
			start: (controller) => {
				void this.#pumpReadable(readable, controller);
			},
			cancel: () => {
				this.close();
			},
		});

		this.writable = new WritableStream<EpoxyWSChunk>({
			write: (chunk) => this.#queueWrite(() => this.#writeChunk(chunk)),
			close: () => this.#queueWrite(() => this.#sendClose()),
			abort: (reason) => this.#writer.abort(reason),
		});
	}

	close(closeInfo: EpoxyWSCloseInfo = {}): void {
		void this.#queueWrite(() => this.#sendClose(closeInfo));
	}

	#queueWrite<T>(op: () => Promise<T>): Promise<T> {
		let run = this.#writeQueue.then(op, op);
		this.#writeQueue = run.then(
			() => undefined,
			() => undefined
		);
		return run;
	}

	async #writeChunk(chunk: EpoxyWSChunk): Promise<void> {
		if (typeof chunk === "string") {
			await this.#writer.write(WsWriteEvent.text(chunk));
			return;
		}

		if (!(chunk instanceof Uint8Array)) {
			throw new TypeError("WebSocket writes must be string or Uint8Array");
		}

		await this.#writer.write(WsWriteEvent.binary(chunk));
	}

	async #sendClose(closeInfo: EpoxyWSCloseInfo = {}): Promise<void> {
		if (this.#closeSent) {
			return;
		}
		this.#closeSent = true;

		let { closeCode, reason } = closeInfo;
		if (
			closeCode !== undefined &&
			(!Number.isInteger(closeCode) || closeCode < 0 || closeCode > 65535)
		) {
			throw new TypeError("closeCode must be an integer between 0 and 65535");
		}
		if (reason !== undefined && typeof reason !== "string") {
			throw new TypeError("reason must be a string");
		}

		await this.#writer.write(WsWriteEvent.close(closeCode, reason));
		await this.#writer.close();
	}

	#resolveClosedOnce(info: EpoxyWSCloseInfo): void {
		if (this.#closedSettled) {
			return;
		}
		this.#closedSettled = true;
		this.#resolveClosed(info);
	}

	#rejectClosedOnce(error: unknown): void {
		if (this.#closedSettled) {
			return;
		}
		this.#closedSettled = true;
		this.#rejectClosed(error);
	}

	async #pumpReadable(
		readable: ReadableStream<WsReadEvent>,
		controller: ReadableStreamDefaultController<EpoxyWSChunk>
	): Promise<void> {
		let reader = readable.getReader();

		try {
			while (true) {
				let { done, value } = await reader.read();
				if (done) {
					this.#resolveClosedOnce({});
					controller.close();
					break;
				}

				let event = value;

				if (event.type === "text") {
					controller.enqueue(event.data);
					continue;
				}
				if (event.type === "binary") {
					controller.enqueue(event.data);
					continue;
				}

				this.#resolveClosedOnce({
					closeCode: event.code,
					reason: event.reason,
				});
				controller.close();
				break;
			}
		} catch (error) {
			this.#rejectClosedOnce(error);
			controller.error(error);
		} finally {
			reader.releaseLock();
		}
	}
}
