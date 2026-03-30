let decoder = new TextDecoder();
let encoder = new TextEncoder();

export function decode(buf: Uint8Array): string {
	return decoder.decode(buf);
}

export function encode(buf: string): Uint8Array {
	return encoder.encode(buf);
}

export type EpoxyRawHeaders = Record<string, string[]>;
export type EpoxyResponse = Response & { rawHeaders: EpoxyRawHeaders };
