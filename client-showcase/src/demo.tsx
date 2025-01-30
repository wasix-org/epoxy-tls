import { fetch as epoxyFetch } from "./epoxy";
import { logger } from "./loggingws";
import { settings } from "./store";

const REQUEST_METHODS = ["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH"];
const REQUEST_BODY_METHODS = ["POST", "PUT", "CONNECT", "PATCH"];

type EpoxyRequest = {
	method: string,
	url: string,
	headers: Record<string, string>,
	body: string,
};
type EpoxyResponse = {
	status: number,
	statusText: string,
	headers: Record<string, string>,
	body: string,
}

async function fetchWithEpoxy(req: EpoxyRequest): Promise<EpoxyResponse> {
	const extra: any = { method: req.method, headers: req.headers, };
	if (req.body) {
		extra.body = req.body;
	}
	const res = await epoxyFetch(req.url, extra);
	return {
		status: res.status,
		statusText: res.statusText,
		/* @ts-ignore */
		headers: res.rawHeaders,
		body: await res.text(),
	};
}

const Option: Component<{ value: string }> = function() {
	return <option value={this.value}>{this.value}</option>
}

const Textarea: Component<{ value: string, id: string, placeholder: string }> = function() {
	return <textarea
		spellcheck="false"
		autocomplete="off"
		autocapitalize="off"
		id={this.id}
		placeholder={this.placeholder}
		bind:value={use(this.value)}
	/>
}

const RequestBuilder: Component<{
	req: EpoxyRequest,
	area: string,
}, {
	method: string,
	url: string,
	headers: string,
	body: string,
}> = function() {
	this.css = `
		background: var(--surface1);
		padding: 1rem;

		font-family: monospace;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;

		.first-line {
			display: flex;
			gap: 0.5rem;
			align-items: center;
		}
		.first-line .url {
			flex: 1;
		}
		.first-line .method, .first-line .url {
			background: transparent;
			color: var(--fg);
			font-family: monospace;
			border: none;
		}
		.first-line .method option {
			background: var(--surface2);
			border: none;
		}

		textarea {
			background: transparent;
			color: var(--fg);
			border: none;
			resize: none;
			flex: 1;
		}
		#body {
			border-top: 2px solid var(--surface6);
		}
	`;

	this.method = "GET";
	this.url = "https://example.com/";
	this.headers = [
		"X-Goog-Spatula: qwertyuiop",
	].join("\n");
	this.body = "";

	useChange([this.method, this.url, this.headers, this.body, settings.wispServer], () => {
		const headers = this.headers.trim()
			.split("\n")
			.map(x => x.split(": "))
			.filter(x => x.length >= 2)
			.map(x => [x[0], x.slice(1).join(": ")]);
		this.req = {
			method: this.method,
			url: this.url,
			headers: Object.fromEntries(headers),
			body: this.body,
		}
	});

	return (
		<div area={this.area}>
			<div class="first-line">
				Wisp Server:
				<input type="url" spellcheck="false" autocomplete="off" autocapitalize="off" class="url" id="wisp" bind:value={use(settings.wispServer)} />
			</div>
			<div class="first-line">
				<select class="method" id="method" bind:value={use(this.method)}>
					{REQUEST_METHODS.map(x => <Option value={x} />)}
				</select>
				<input type="url" spellcheck="false" autocomplete="off" autocapitalize="off" class="url" id="url" bind:value={use(this.url)} />
				<span>HTTP/1.1</span>
			</div>
			<Textarea bind:value={use(this.headers)} id="headers" placeholder="Headers" />
			{$if(use(this.method, x => REQUEST_BODY_METHODS.includes(x)),
				<Textarea bind:value={use(this.body)} id="body" placeholder="Body" />
			)}
		</div>
	);
}

const ResponseView: Component<{ response: EpoxyResponse }> = function() {
	this.css = `
		font-family: monospace;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;

		.first-line {
			display: flex;
			gap: 0.5rem;
			align-items: center;
		}
		.body {
			white-space: pre-wrap;
		}

		.divider {
			height: 0.5rem;
		}

		.warning {
			font-family: sans-serif;
			color: var(--error);
		}
	`;

	const corsHeader = Object.entries(this.response.headers).find(x => x[0] === "access-control-allow-origin");
	const corsFailed = !corsHeader || !corsHeader[1].includes("*");

	return (
		<div>
			{corsFailed ? <div class="warning">This request would have failed with native fetch</div> : null}
			<div class="first-line">
				<span>HTTP/1.1</span>
				<span>{this.response.status}</span>
				<span>{this.response.statusText}</span>
			</div>
			<div class="headers">
				{Object.entries(this.response.headers).map(([k, v]) => {
					return <div>{k}: {v}</div>;
				})}
			</div>
			<div class="divider" />
			<div class="body">
				{this.response.body}
			</div>
		</div>
	)
}

const RequestSender: Component<{ req: EpoxyRequest, area: string }, {
	loading: boolean,
	id: number,
	response: EpoxyResponse | null,
	error: Error | null,
}> = function() {
	this.css = `
		background: var(--surface1);
		padding: 1rem;

		overflow-y: scroll;
		
		.error {
			font-family: monospace;
		}
	`;

	this.id = 0;
	this.loading = false;
	this.response = null;
	this.error = null;

	const sendReq = async (req: EpoxyRequest, id: number) => {
		try {
			const res = await fetchWithEpoxy(req);
			if (this.id === id) {
				this.response = res;
				console.debug("successfully fetched:", req, res);
				return true;
			}
		} catch (err) {
			if (this.id === id) {
				this.error = err as Error;
				console.log("error while fetching:", req, err);
				return true;
			}
		}
		return false;
	};

	useChange([this.req], async () => {
		this.loading = true;
		this.response = null;
		this.error = null;

		const id = ++this.id;
		if (await sendReq(this.req, id)) {
			this.loading = false;
		}
	});

	return (
		<div area={this.area}>
			{$if(use(this.loading),
				<div>
					Loading...
				</div>
			)}
			{use(this.response, x => x ? <ResponseView response={x!} /> : null)}
			{use(this.error, x => x ? <div class="error">{x}</div> : null)}
		</div>
	)
}

const WebsocketLogger: Component<{ area: string }, {}> = function() {
	this.css = `
		display: flex;
		flex-direction: column;

		overflow-y: scroll;
		overflow-x: hidden;
		max-height: 100%;
		gap: 4px;
		font-family: monospace;

		background-color: var(--surface0);

		.message {
			display: grid;
			grid-template-columns: 2rem calc(50% - 2rem - 6px) 50%;
			align-items: stretch;
			gap: 4px;
			min-width: 0;
		}

		.type {
			display: flex;
			justify-content: center;
			padding-top: 0.25rem;
		}

		.type-rx {
			background: var(--success);
			color: var(--bg);
		}
		.type-tx {
			background: var(--error);
			color: var(--bg);
		}

		.payload {
			white-space: pre-wrap;
			padding: 0.5rem;
			background-color: var(--surface1);
		}

		.text {
			line-break: anywhere;
		}
	`;

	return (
		<div area={this.area}>
			{use(logger.logged, x => {
				let ret = x.map(x => {
					return (
						<div class="message">
							<div class={`type type-${x.type}`}>
								{x.type}
							</div>
							<div class="payload binary">
								{x.hex}
							</div>
							<div class="payload text">
								{x.text}
							</div>
						</div>
					)
				});
				return ret;
			})}
		</div>
	);
}

export const Demo: Component<{}, {
	req: EpoxyRequest,
}> = function() {
	this.css = `
		display: grid;
		grid-template-areas:
			"a b"
			"c c";
		grid-auto-rows: 24rem;
		grid-auto-columns: 1fr 1fr;
		gap: 4px;

		div[area="a"] { grid-area: a; }
		div[area="b"] { grid-area: b; }
		div[area="c"] { grid-area: c; }
	`;

	return (
		<div>
			<RequestBuilder bind:req={use(this.req)} area="a" />
			<RequestSender req={use(this.req)} area="b" />
			<WebsocketLogger area="c" />
		</div>
	)
}
