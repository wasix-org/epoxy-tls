import wbgInit from "epoxy/wbg";

export type EpoxyInitInput =
	| RequestInfo
	| URL
	| Response
	| BufferSource
	| WebAssembly.Module
	| Promise<RequestInfo | URL | Response | BufferSource | WebAssembly.Module>
	| {
			module_or_path:
				| RequestInfo
				| URL
				| Response
				| BufferSource
				| WebAssembly.Module
				| Promise<
						RequestInfo | URL | Response | BufferSource | WebAssembly.Module
				  >;
	  };

export async function init(input: EpoxyInitInput) {
	await wbgInit(
		typeof input === "object" && input !== null && "module_or_path" in input
			? input
			: { module_or_path: input }
	);
}

export * from "./public";
