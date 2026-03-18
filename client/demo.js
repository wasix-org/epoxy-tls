import {
	init,
	version,
	EpoxyClient,
	JsProtocolExtension,
	JsProtocolExtensionBuilder,
	WebSocketJsProvider,
	WispSocketProvider,
} from "./dist/epoxy.js";

console.log(version);

await init();

class PasswordExt extends JsProtocolExtension {
	toSend;
	required;

	constructor(required, toSend) {
		super(0x02, [], []);
		this.toSend = toSend;
		this.required = required;
	}

	encode() {
		if (this.toSend) {
			let [_user, _pw] = this.toSend;
			let user = new TextEncoder().encode(_user);
			let pw = new TextEncoder().encode(_pw);

			let arr = new Uint8Array(3 + user.byteLength + pw.byteLength);
			arr[0] = user.byteLength;
			new DataView(arr.buffer).setUint16(1, pw.byteLength, true);
			arr.set(user, 3);
			arr.set(pw, 3 + user.byteLength);

			return arr;
		}
		return new Uint8Array();
	}
}

class PasswordExtBuilder extends JsProtocolExtensionBuilder {
	toSend;

	constructor(toSend) {
		super(0x02);

		this.toSend = toSend;
	}

	buildFromBytes(bytes) {
		console.log("password required:", bytes[0]);
		return new PasswordExt(bytes[0] !== 0);
	}

	buildToExtension() {
		return new PasswordExt(undefined, this.toSend);
	}
}

let wisptoken = localStorage["wisptoken"];

const provider = new WispSocketProvider(
	new WebSocketJsProvider(),
	/*
	"wss://puter.cafe/",
	() => [{ builders: [new PasswordExtBuilder(["", wisptoken])] }, [0x02]]
	*/
	"wss://anura.pro/"
);

await provider.replaceMux();

/*
let exts = await provider.getExtensions();
console.log(exts.get(0));
exts.drop();
*/

const client = new EpoxyClient(provider);

let ret = await client.fetch("https://httpbin.org/post", {
	method: "POST",
	body: JSON.stringify({ a: "b" }),
	headers: {
		"Content-Type": "application/json",
		"x-AbC": "abc",
		"X-BbC": "bcd",
	},
});
console.log(ret);
console.log(ret.rawHeaders);
console.log(await ret.text());

async function sendTest(read, write, log, data) {
	let j = (x) => JSON.stringify(x);
	console.log(`sending ${j(data)} over ${log}`);
	await write.getWriter().write(new TextEncoder().encode(data));
	console.log(`sent ${j(data)} over ${log}`);
	let decoder = new TextDecoder();
	let received = "";
	let got = false;
	for await (let chunk of read) {
		let decoded = decoder.decode(chunk, { stream: true });
		received += decoded;
		console.log(`decoded ${j(decoded)} over ${log}`);
		if (received.includes(data)) {
			got = true;
			break;
		}
	}
	if (!got)
		throw new Error(
			`failed to get ${j(data)} over ${log} (received ${j(received)})`
		);
}

let tcp = await client.connect("tcpbin.com", 4242);
await sendTest(tcp.read, tcp.write, "tcp", ":3\n");

let tls = await client.connectTls("tcpbin.com", 4243);
await sendTest(tls.read, tls.write, "tls", ":3\n");

console.log("done");
