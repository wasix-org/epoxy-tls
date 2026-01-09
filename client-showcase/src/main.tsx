
import { css, type Component } from "dreamland/core";
import settings from "./store";

import Demo from "./demo";
import Git from "./components/git";
import Link from "./components/link"
import Theme from "./components/theme"

const App: Component = function() {
	return (
		<div id="app" class={use(settings.theme)}>
			<div class="root">
				<h1><code>epoxy-tls</code></h1>
				<div>
					<Git repo="mercuryworkshop/epoxy-tls"><i>GitHub</i></Git>
					{' | '}
					<Link href="https://www.npmjs.com/package/@mercuryworkshop/epoxy-tls"><i>npm</i></Link>
					{' | '}
					<Link href="https://crates.io/crates/wisp-mux"><i>crates.io</i></Link>
					{' | '}
					<Theme />
				</div>
				<p>
					A set of libraries and programs for securely bypassing CORS end-to-end-encrypted by running TLS in the browser through <Link href="https://webassembly.org"><i>WebAssembly</i></Link>.
					The client is built in Rust with the <Git repo="rustls/rustls"><code>rustls</code></Git> TLS implementation and <Git repo="hyperium/hyper"><code>hyper</code></Git> HTTP implementation.
					It uses the <Git repo="mercuryworkshop/wisp-protocol">Wisp protocol</Git> as a TCP proxy to connect to and communicate with remote servers.
				</p>
				<h2>Demo</h2>
				<Demo />
				<h2>Features</h2>
				<ul>
					<li>A blazingly fast Wisp server: <code>epoxy-server</code></li>
					<ul>
						<li>As of January 2025, the fastest Wisp server measured in <Git repo="mercuryworkshop/wispmark">Wispmark</Git>, a benchmarking tool for Wisp servers, achieving <b>4.3GiB/s throughput</b> with 5 clients connecting with 10 streams each</li>
						<li>Very configurable, with support for speed limits, domain blacklist/whitelist, statistics over HTTP, and listening on TCP, Unix sockets, and files all in a single TOML/JSON/YAML file</li>
						<li>Has robust logging, showing every client connected and every stream created</li>
						<li>Supports the <Git repo="mercuryworkshop/wispnet-protocol/tree/epoxy-server">WispNet</Git> protocol extension, allowing for peer-to-peer communication over Wisp</li>
						<li>Supports the <code>twisp</code> protocol extension, allowing for terminals to be multiplexed over Wisp</li>
					</ul>
					<li>A CORS bypassing fetch and WebSocket implementation for the web: <code>epoxy-client</code></li>
					<ul>
						<li>Very small: the WASM is 780K uncompressed and 304K compressed (minimal version)</li>
						<li>Supports HTTP/1.1 and HTTP/2 for increased performance</li>
						<li>Supports WebSockets through the <Git repo="denoland/fastwebsockets"><i>fastwebsockets</i></Git> library</li>
						<li>Supports HTTP/2 full-duplex, allowing for streaming requests and responses at the same time</li>
					</ul>
					<li>A Rust crate implementing the Wisp protocol</li>
					<ul>
						<li>Fully async, benefitting from the IO optimizations of popular async runtimes</li>
						<li>Built to be runtime and IO agnostic, allowing for use in non-Tokio environments</li>
						<li>Multiplexed streams implement the familiar <Link href="https://docs.rs/futures"><i>futures</i></Link> IO traits like <code>Stream&lt;Item = Bytes&gt; + Sink&lt;Bytes&gt;</code> and <code>AsyncRead + AsyncWrite</code></li>
						<ul>
							<li>This allows for multiplexed streams to be easily plugged into other popular crates like <Link href="https://docs.rs/hyper"><code>hyper</code></Link> and <Link href="https://docs.rs/tokio-tungstenite"><code>async-tungstenite</code></Link></li>
						</ul>
						<li>Supports custom protocol extensions, allowing you to quickly add on authentication and other features to the existing Wisp protocol</li>
					</ul>
				</ul>
				<h2>Licenses</h2>
				<ul>
					<li><code>epoxy-client</code>: AGPL</li>
					<li><code>epoxy-server</code>: AGPL</li>
					<li><code>wisp-mux</code>: MIT</li>
				</ul>
			</div>
		</div>
	);
}
App.style = css`
	:scope {
		background: var(--bg-sub);
		color: var(--fg);

		padding: 1rem;
		overflow: scroll;
	}
	.root {
		margin: auto;
		padding: 1rem;
		max-width: 64rem;
	}
`;

const root = document.getElementById("app")!;
try {
	root.replaceWith(<App />);
} catch (err) {
	root.replaceWith(document.createTextNode(`Error while rendering: ${err}`));
}
