import { defineConfig } from "rollup";
import fs from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import process from "node:process";

import terser from "@rollup/plugin-terser";
import typescript from "@rollup/plugin-typescript";
import dts from "rollup-plugin-dts";
import nodeResolve from "@rollup/plugin-node-resolve";
import wasm from "@rollup/plugin-wasm";

let DEBUG = false;
let SIZE = false;

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

async function compileRust(folder, args) {
	await fs.mkdir(folder, { recursive: true });

	await run(
		"cargo",
		[
			"build",
			"--release",
			"--target",
			"wasm32-unknown-unknown",
			"-Zbuild-std=std",
			"-Zbuild-std-features=optimize_for_size",
			...args,
		],
		{
			...(SIZE ? {} : { CFLAGS: "-O3" }),
			RUSTFLAGS:
				"-Zunstable-options -Cpanic=immediate-abort -Zlocation-detail=none -Zfmt-debug=none",
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

	await run("wasm-opt", [
		path.join(folder, "epoxy_client_bg.wasm"),
		"-o",
		path.join(folder, "epoxy.wasm"),
		...(DEBUG ? ["-g"] : []),
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

	const stat = await fs.stat(path.join(folder, "epoxy.wasm"));
	const size = stat.size / 1024;
	console.log(`built rust in "${folder}" with size ${size.toFixed(2)}kb`);
}

function rust(folderName, args = []) {
	const folder = path.join(import.meta.dirname, "build", folderName);
	return {
		name: "rust",
		buildStart: async () => {
			await compileRust(folder, args);
		},
		resolveId: (source) => {
			if (source === "epoxy/wbg") return path.join(folder, "epoxy_client.js");
			if (source === "epoxy/wasm") return path.join(folder, "epoxy.wasm");
			return null;
		},
	};
}

const rustFallback = {
	name: "rust-fallback",
	resolveId: (source) => {
		if (source.startsWith("epoxy/"))
			throw new Error(`${source} should not be leaking`);
		return null;
	},
};

const common = (include) => [
	nodeResolve(),
	wasm({
		maxFileSize: 2 * 1024 * 1024,
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

const cfg = (inputDir, inputFile, output, defs, plugins) => {
	const input = path.join(inputDir, inputFile);
	const out = [
		defineConfig({
			input,
			output: [{ file: output, sourcemap: true, format: "es" }],
			plugins: [common(inputDir), ...plugins, rustFallback],
		}),
	];
	if (defs) {
		out.push(
			defineConfig({
				input:
					"dist/types/" + input.substring("js/".length).replace(".ts", ".d.ts"),
				output: [{ file: output.replace(".js", ".d.ts"), format: "es" }],
				plugins: [dts(), rustFallback],
			})
		);
	}
	return out;
};

export default (args) => {
	if (args["config-debug"]) DEBUG = true;
	if (args["config-size"]) SIZE = true;

	return defineConfig([
		...cfg("js", "index.ts", "dist/epoxy.js", true, [rust("full")]),
	]);
};
