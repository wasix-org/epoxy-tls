export function writeTransform<I, O>(
	write: WritableStream<I>,
	transformer: (val: O) => Promise<I>
): WritableStream<O> {
	const writer = write.getWriter();
	return new WritableStream({
		async write(val, _) {
			writer.write(await transformer(val));
		},
		close() {
			writer.close();
		},
	});
}
