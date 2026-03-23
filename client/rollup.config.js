import { defineConfig } from "rollup";
import fs from "node:fs/promises";
import path from "node:path";
import { spawn, exec as processExec } from "node:child_process";
import process from "node:process";

import terser from "@rollup/plugin-terser";
import typescript from "@rollup/plugin-typescript";
import dts from "rollup-plugin-dts";
import nodeResolve from "@rollup/plugin-node-resolve";
import wasm from "@rollup/plugin-wasm";

let VERSION;
let DEV = false;
let SIZE = false;
const INLINE_WASM_LIMIT = 2 * 1024 * 1024;
const WASM_BINDGEN_IMPORT_META_FALLBACK =
	"module_or_path = new URL('epoxy_client_bg.wasm', import.meta.url);";
let FAKED_TYPES = [
	"JsProvider",
	"WispProvider",
	"WasmProvider",
	"WasmWispProvider",
	"JsWispV2Handshake",
	"ProtocolExtensionBuilders",
	"WispProtocolExtensions",
	"JsProtocolExtensionBuilder",
	"JsProtocolExtension",
	"JsTransportRead",
	"JsTransportWrite",
	"UdpProtocolExtensionBuilder",
	"UdpProtocolExtensionBuilderRef",
	"UdpProtocolExtension",
	"MotdProtocolExtensionBuilder",
	"MotdProtocolExtensionBuilderRef",
	"MotdProtocolExtension",
	"PasswordProtocolExtensionBuilder",
	"PasswordProtocolExtensionBuilderRef",
	"PasswordProtocolExtension",
	"CertAuthProtocolExtensionBuilder",
	"CertAuthProtocolExtensionBuilderRef",
	"CertAuthProtocolExtension",
	"WsReadEvent",
	"WsWriteEvent",
];
const rustBuildCache = new Map();

async function run(cmd, args, env = {}) {
	console.log(
		Object.entries(env)
			.map(([k, v]) => `${k}="${v}"`)
			.join(" ") +
			" " +
			[cmd, ...args].join(" ")
	);
	let res, rej;
	const promise = new Promise((a, b) => {
		res = a;
		rej = b;
	});

	const start = performance.now();
	const spawned = spawn(cmd, args, {
		env: Object.assign({}, process.env, env),
		stdio: "inherit",
	});
	spawned.on("error", (x) => rej(x));
	spawned.on("exit", (x) => {
		const time = performance.now() - start;
		console.log(`ran in ${time.toFixed(0)}ms`);

		if (!x) {
			res();
		} else {
			rej(new Error(`process exited with code ${x}`));
		}
	});

	await promise;
	spawned.kill();
}

async function compileRust(folder, features) {
	await fs.mkdir(folder, { recursive: true });
	const cargoFeatures = [...features, ...(DEV ? ["debug"] : [])];
	const featureArgs = ["--no-default-features"];
	if (cargoFeatures.length > 0) {
		featureArgs.push("--features", cargoFeatures.join(","));
	}

	await run(
		"cargo",
		[
			"build",
			"--release",
			"--target",
			"wasm32-unknown-unknown",
			...featureArgs,
			"-Zbuild-std=std",
			"-Zbuild-std-features=optimize_for_size",
		],
		{
			...(SIZE ? {} : { CFLAGS: "-O3" }),
			RUSTFLAGS:
				"-Zunstable-options -Cpanic=immediate-abort" +
				(DEV ? "" : " -Zlocation-detail=none -Zfmt-debug=none"),
		}
	);

	await run("wasm-bindgen", [
		"--target",
		"web",
		"--out-dir",
		folder,
		path.join(
			"..",
			"target",
			"wasm32-unknown-unknown",
			"release",
			"epoxy_client.wasm"
		),
	]);

	const bindgenPath = path.join(folder, "epoxy_client.js");
	let bindgen = await fs.readFile(bindgenPath, "utf8");
	if (!bindgen.includes(WASM_BINDGEN_IMPORT_META_FALLBACK)) {
		throw new Error("wasm-bindgen init fallback not found");
	}
	bindgen = bindgen.replace(
		WASM_BINDGEN_IMPORT_META_FALLBACK,
		"throw new Error('module_or_path must be provided when calling init()');"
	);
	await fs.writeFile(bindgenPath, bindgen);

	await run("wasm-opt", [
		path.join(folder, "epoxy_client_bg.wasm"),
		"-o",
		path.join(folder, "epoxy.wasm"),
		...(DEV ? ["-g"] : []),
		"--converge",
		"-Oz",
		"--flatten",
		"--rereloop",
		"-Oz",
		"--vacuum",
		"--dce",
		"-Oz",
		"--gufa",
		"-Oz",
	]);

	const wasmPath = path.join(folder, "epoxy.wasm");
	const wasmOut = path.join(
		import.meta.dirname,
		"dist",
		`${path.basename(folder)}.wasm`
	);
	await fs.mkdir(path.dirname(wasmOut), { recursive: true });
	await fs.copyFile(wasmPath, wasmOut);

	const stat = await fs.stat(wasmPath);
	const size = stat.size / 1024;
	console.log(`built rust in "${folder}" with size ${size.toFixed(2)}kb`);
}

function rust(folderName, features = []) {
	const folder = path.join(import.meta.dirname, "build", folderName);
	return {
		name: "rust",
		async buildStart() {
			const cacheKey = JSON.stringify({ folder, features, DEV, SIZE });
			let cached = rustBuildCache.get(cacheKey);
			if (!cached) {
				cached = compileRust(folder, features);
				rustBuildCache.set(cacheKey, cached);
			}

			await cached;
		},
		resolveId(source) {
			if (source === "epoxy/wbg") return path.join(folder, "epoxy_client.js");
			if (source === "epoxy/wasm") return path.join(folder, "epoxy.wasm");
			return null;
		},
	};
}

const versionInfo = {
	name: "epx-version",
	resolveId(source) {
		if (source === "epoxy/version") return "\0epoxy/version";
		return null;
	},
	load(source) {
		if (source === "\0epoxy/version")
			return `export default ${JSON.stringify(VERSION)}`;
		return null;
	},
};

const rustFallback = {
	name: "rust-fallback",
	resolveId(source) {
		if (source === "epoxy/wbg") return "\0epoxy/wbg";
		if (source.startsWith("epoxy/"))
			throw new Error(`${source} should not be leaking`);
		return null;
	},
	load(id) {
		if (id === "\0epoxy/wbg") {
			return [
				"export default async function wbgInit() {}",
				...FAKED_TYPES.map((x) => `export class ${x} {}`),
			].join("\n");
		}
		return null;
	},
};

const common = (include, bundleWasm) => [
	nodeResolve(),
	wasm({
		maxFileSize: bundleWasm ? INLINE_WASM_LIMIT : 0,
	}),
	typescript({
		include: include + "/**/*",
		filterRoot: process.cwd(),
	}),
	terser({
		parse: {},
		compress: {
			passes: 4,
			hoist_funs: true,
		},
		keep_classnames: /^EpoxyError$/,
		//mangle: {
		//	keep_classnames: false,
		//	keep_fnames: false,
		//	properties: {
		//		regex: /^_.*/,
		//	},
		//},
		format: {
			wrap_func_args: false,
		},
		module: true,
		ie8: false,
		safari10: false,
		ecma: 5,
	}),
];

const cfg = (inputDir, inputFile, output, defs, bundleWasm, plugins) => {
	const input = path.join(inputDir, inputFile);
	const out = [
		defineConfig({
			input,
			output: [{ file: output, sourcemap: true, format: "es" }],
			plugins: [
				common(inputDir, bundleWasm),
				...plugins,
				versionInfo,
				rustFallback,
			],
		}),
	];
	if (defs) {
		out.push(
			defineConfig({
				input:
					"dist/types/" + input.substring("js/".length).replace(".ts", ".d.ts"),
				output: [{ file: output.replace(".js", ".d.ts"), format: "es" }],
				plugins: [dts(), versionInfo, rustFallback],
			})
		);
	}
	return out;
};

export default async (args) => {
	if (args["config-dev"]) DEV = true;
	if (args["config-size"]) SIZE = true;

	let git = await new Promise((res, rej) => {
		processExec("git describe --always --dirty", (err, stdout) => {
			if (err) rej(err);
			else if (stdout) res(stdout.trim());
		});
	});

	let version = JSON.parse(await fs.readFile("package.json", "utf8")).version;

	VERSION = { version, git };

	const profiles = ["minimal", "full"];
	// const profiles = ["minimal", "full", "extended"];
	const variants = [
		{ bundled: false, inputFile: "index_unbundled.ts", suffix: "" },
		{ bundled: true, inputFile: "index.ts", suffix: ".bundled" },
	];

	const outputs = [];
	for (let profile of profiles) {
		const rustPlugin = rust(profile, [profile]);
		for (let variant of variants) {
			outputs.push(
				...cfg(
					"js",
					variant.inputFile,
					`dist/${profile}${variant.suffix}.js`,
					true,
					variant.bundled,
					[rustPlugin]
				)
			);
		}
	}

	return defineConfig(outputs);
};
