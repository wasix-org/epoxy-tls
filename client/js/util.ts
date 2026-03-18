let decoder = new TextDecoder();
let encoder = new TextEncoder();

export function decode(buf: Uint8Array): string {
	return decoder.decode(buf);
}

export function encode(buf: string): Uint8Array {
	return encoder.encode(buf);
}
