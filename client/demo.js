import {
	EpoxyClient,
	init,
	WebSocketJsProvider,
	WispSocketProvider,
} from "./dist/epoxy.js";

await init();

const provider = new WispSocketProvider(
	new WebSocketJsProvider(),
	"wss://anura.pro/"
);

const client = new EpoxyClient(provider);

let ret = await client.fetch("https://httpbin.org/post", {
	method: "POST",
	body: JSON.stringify({ a: "b" }),
	headers: { "Content-Type": "application/json" },
});
console.log(await ret.json());
console.log("done");
