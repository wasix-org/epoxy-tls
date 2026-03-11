let decoder = new TextDecoder();

export function decode(buf: Uint8Array): string {
	return decoder.decode(buf);
}
