import { defineConfig } from "vite";
import { viteStaticCopy } from "vite-plugin-static-copy";

export default defineConfig({
	base: './',
	plugins: [
		viteStaticCopy({
			targets: [
				{ src: "node_modules/@mercuryworkshop/epoxy-tls/minimal/epoxy.wasm", dest: "epoxy" },
			]
	})],
	build: {
		target: "es2022",
	}
});
