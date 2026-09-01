use std::{fmt::Display, future::Future, io::Cursor};

use anyhow::Context;
use bytes::Bytes;
use futures_util::future::Either;
use http_body_util::Full;
use hyper::{
	body::Incoming,
	header::{CACHE_CONTROL, CONTENT_TYPE, HOST, SEC_WEBSOCKET_PROTOCOL},
	server::conn::http1::Builder,
	service::service_fn,
	upgrade::OnUpgrade,
	HeaderMap, Request, Response, StatusCode, Uri,
};
use hyper_util::rt::TokioIo;
use log::{debug, error, trace};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use tokio_websockets::Limits;
use wisp_mux::ws::{
	TokioWebsocketsTransport, TransportExt, WebSocketSplitRead, WebSocketSplitWrite,
};

use crate::{
	config::SocketTransport,
	generate_stats,
	listener::{ServerStream, ServerStreamExt, ServerStreamRead, ServerStreamWrite},
	stream::WebSocketStreamWrapper,
	upgrade::{is_upgrade_request, upgrade},
	util_chain::{chain, Chain},
	util_map_err::MapErr,
	CONFIG,
};

pub type WispStreamRead = Either<
	WebSocketSplitRead<TokioWebsocketsTransport<Chain<Cursor<Bytes>, ServerStream>>>,
	MapErr<FramedRead<ServerStreamRead, LengthDelimitedCodec>>,
>;
pub type WispWsStreamWrite =
	WebSocketSplitWrite<TokioWebsocketsTransport<Chain<Cursor<Bytes>, ServerStream>>>;
pub type WispStreamWrite =
	Either<WispWsStreamWrite, MapErr<FramedWrite<ServerStreamWrite, LengthDelimitedCodec>>>;
pub type WispResult = (WispStreamRead, WispStreamWrite);

pub enum ServerRouteResult {
	Wisp {
		stream: WispResult,
		has_ws_protocol: bool,
	},
	Wispnet {
		stream: WispResult,
	},
	WsProxy {
		stream: WebSocketStreamWrapper,
		path: String,
		udp: bool,
	},
}

impl Display for ServerRouteResult {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Wisp { .. } => write!(f, "Wisp"),
			Self::Wispnet { .. } => write!(f, "Wispnet"),
			Self::WsProxy { path, udp, .. } => write!(f, "WsProxy path {path:?} udp {udp:?}"),
		}
	}
}

type Body = Full<Bytes>;

fn html_escape(value: &str) -> String {
	let mut escaped = String::with_capacity(value.len());
	for character in value.chars() {
		match character {
			'&' => escaped.push_str("&amp;"),
			'<' => escaped.push_str("&lt;"),
			'>' => escaped.push_str("&gt;"),
			'\"' => escaped.push_str("&quot;"),
			'\'' => escaped.push_str("&#39;"),
			_ => escaped.push(character),
		}
	}
	escaped
}

fn first_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
	get_header(headers, name)
		.and_then(|value| {
			value
				.split(',')
				.next()
				.map(str::trim)
				.map(ToString::to_string)
		})
		.filter(|value| !value.is_empty())
}

fn websocket_url(req: &Request<Incoming>) -> String {
	let headers = req.headers();
	let forwarded_proto = first_header_value(headers, "x-forwarded-proto");
	let forwarded_tls = first_header_value(headers, "x-forwarded-ssl");
	let secure = forwarded_proto.as_deref().is_some_and(|value| {
		value.eq_ignore_ascii_case("https") || value.eq_ignore_ascii_case("wss")
	}) || forwarded_tls
		.as_deref()
		.is_some_and(|value| value.eq_ignore_ascii_case("on"));
	let scheme = if secure { "wss" } else { "ws" };
	let host = first_header_value(headers, "x-forwarded-host")
		.or_else(|| {
			headers
				.get(HOST)
				.and_then(|value| value.to_str().ok())
				.map(ToString::to_string)
		})
		.unwrap_or_else(|| "localhost:4000".to_string());
	let prefix = CONFIG.wisp.prefix.trim_end_matches('/');
	let endpoint = if prefix.is_empty() {
		"/".to_string()
	} else if prefix.starts_with('/') {
		format!("{prefix}/")
	} else {
		format!("/{prefix}/")
	};
	format!("{scheme}://{host}{endpoint}")
}

fn autoconfigure_target() -> Option<(String, String)> {
	let configured = CONFIG.server.autoconfigure_domain.as_deref()?.trim();
	if configured.is_empty() {
		return None;
	}
	let target = if configured.contains("://") {
		configured.to_string()
	} else {
		format!("https://{configured}")
	};
	let uri = target.parse::<Uri>().ok()?;
	let scheme = uri.scheme_str()?;
	if scheme != "http" && scheme != "https" {
		return None;
	}
	let authority = uri.authority()?.to_string();
	Some((target, authority))
}

fn index_resp(req: &Request<Incoming>) -> anyhow::Result<Response<Body>> {
	let raw_url = websocket_url(req);
	let url = html_escape(&raw_url);
	let autoconfigure = autoconfigure_target();
	let autoconfigure_link = if let Some((bridge_url, label)) = autoconfigure.as_ref() {
		format!(
			r#"<a id="autoconfigure" class="autoconfigure" href="{}"><span class="pulse"></span>Automatically connect to {}</a>"#,
			html_escape(bridge_url),
			html_escape(label),
		)
	} else {
		String::new()
	};
	let autoconfigure_script = if let Some((bridge_url, _)) = autoconfigure {
		let bridge_url = serde_json::to_string(&bridge_url)?;
		let endpoint = serde_json::to_string(&raw_url)?;
		format!(
			r#"<script>
  (() => {{
    const bridgeUrl = {bridge_url};
    const endpoint = {endpoint};
    const target = new URL(bridgeUrl);
    target.searchParams.set('endpoint', endpoint);
    document.getElementById('autoconfigure').href = target.href;
  }})();
</script>"#
		)
	} else {
		String::new()
	};
	let html = format!(
		r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>WISP WebSocket server</title>
  <style>
    :root {{ color-scheme: light; font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace; background: #fafafa; color: #111; }}
    * {{ box-sizing: border-box; }}
    body {{ min-height: 100vh; margin: 0; background-image: radial-gradient(#d8d8d8 0.7px, transparent 0.7px); background-size: 16px 16px; }}
    .shell {{ width: min(72rem, calc(100% - 2rem)); min-height: 100vh; margin: 0 auto; padding: 1.25rem 0; display: flex; flex-direction: column; }}
    header {{ display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: 0.9rem 0 1.25rem; font-size: 0.72rem; letter-spacing: 0.08em; text-transform: uppercase; }}
    .brand {{ display: flex; align-items: center; gap: 0.65rem; font-weight: 700; }}
    .mark {{ width: 1.35rem; height: 1.35rem; display: grid; place-items: center; background: #111; color: #fafafa; }}
    .status {{ display: flex; align-items: center; gap: 0.45rem; color: #414141; }}
    .status-dot, .pulse {{ width: 0.5rem; height: 0.5rem; border-radius: 50%; background: #22a06b; box-shadow: 0 0 0 3px #dff5eb; }}
    main {{ flex: 1; display: grid; place-items: center; padding: 4rem 0; }}
    .panel {{ width: min(52rem, 100%); background: #fafafa; border: 1px solid #111; box-shadow: 8px 8px 0 #111; }}
    .hero {{ padding: clamp(2rem, 6vw, 4.5rem); }}
    .eyebrow {{ margin: 0 0 1.1rem; color: #666; font-size: 0.72rem; letter-spacing: 0.1em; text-transform: uppercase; }}
    h1 {{ max-width: 16ch; margin: 0; font-size: clamp(2rem, 6vw, 4.6rem); line-height: 0.98; letter-spacing: -0.055em; }}
    .lede {{ max-width: 40rem; margin: 1.5rem 0 2.25rem; color: #414141; font-size: 0.9rem; line-height: 1.65; }}
    .endpoint-label {{ display: block; margin-bottom: 0.55rem; color: #666; font-size: 0.68rem; letter-spacing: 0.09em; text-transform: uppercase; }}
    .url {{ display: grid; grid-template-columns: minmax(0, 1fr) auto; border: 1px solid #111; background: #fff; }}
    code {{ min-width: 0; overflow-wrap: anywhere; padding: 1rem; color: #111; font: inherit; line-height: 1.4; }}
    button {{ border: 0; border-left: 1px solid #111; border-radius: 0; padding: 0 1.25rem; background: #111; color: #fff; font: inherit; font-size: 0.75rem; font-weight: 700; letter-spacing: 0.05em; text-transform: uppercase; cursor: pointer; }}
    button:hover {{ background: #333; }}
    .autoconfigure {{ display: inline-flex; align-items: center; gap: 0.55rem; margin-bottom: 1.25rem; padding: 0.8rem 1rem; background: #111; color: #fff; font-size: 0.68rem; font-weight: 700; letter-spacing: 0.05em; text-decoration: none; }}
    .autoconfigure:hover {{ background: #333; }}
    .pulse {{ display: inline-block; width: 0.4rem; height: 0.4rem; background: #5ee6a8; box-shadow: none; animation: blink 1.8s infinite; }}
    @keyframes blink {{ 50% {{ opacity: 0.35; }} }}
    @media (max-width: 38rem) {{
      .shell {{ width: min(100% - 1.25rem, 72rem); }}
      .panel {{ box-shadow: 5px 5px 0 #111; }}
      .url {{ grid-template-columns: 1fr; }}
      button {{ min-height: 3rem; border-left: 0; border-top: 1px solid #111; }}
    }}
  </style>
</head>
<body>
  <div class="shell">
    <header>
      <div class="brand"><span class="mark">W</span> WISP SERVER</div>
      <div class="status"><span class="status-dot"></span> ONLINE</div>
    </header>
    <main>
      <section class="panel">
        <div class="hero">
          <p class="eyebrow">Connection established</p>
          {autoconfigure_link}
          <h1>WISP WebSocket server configured!</h1>
          <p class="lede">Your proxy is online. Copy and paste this endpoint into your WISP-compatible client to start routing traffic.</p>
          <label class="endpoint-label" for="wisp-url">WebSocket endpoint</label>
          <div class="url">
            <code id="wisp-url">{url}</code>
            <button id="copy-url" type="button">Copy URL</button>
          </div>
        </div>
      </section>
    </main>
  </div>
  <script>
    document.getElementById('copy-url').addEventListener('click', async (event) => {{
      await navigator.clipboard.writeText(document.getElementById('wisp-url').textContent);
      event.currentTarget.textContent = 'Copied';
    }});
  </script>
  {autoconfigure_script}
</body>
</html>"#
	);
	Ok(Response::builder()
		.status(StatusCode::OK)
		.header(CONTENT_TYPE, "text/html; charset=utf-8")
		.header(CACHE_CONTROL, "no-store")
		.body(Body::new(html.into()))?)
}

fn non_ws_resp(req: &Request<Incoming>) -> anyhow::Result<Response<Body>> {
	if req.uri().path() == "/" {
		return index_resp(req);
	}
	Ok(Response::builder()
		.status(StatusCode::OK)
		.header(CONTENT_TYPE, "text/plain; charset=utf-8")
		.body(Body::new(CONFIG.server.non_ws_response.as_bytes().into()))?)
}

async fn send_stats() -> anyhow::Result<Response<Body>> {
	match generate_stats().await {
		Ok(x) => {
			debug!("sent server stats to http client");
			Ok(Response::builder()
				.status(StatusCode::OK)
				.body(Body::new(x.into()))?)
		}
		Err(x) => {
			error!("failed to send stats to http client: {:?}", x);
			Ok(Response::builder()
				.status(StatusCode::INTERNAL_SERVER_ERROR)
				.body(Body::new(x.to_string().into()))?)
		}
	}
}

fn get_header(headers: &HeaderMap, header: &str) -> Option<String> {
	headers
		.get(header)
		.and_then(|x| x.to_str().ok())
		.map(ToString::to_string)
}

enum HttpUpgradeResult {
	Wisp {
		has_ws_protocol: bool,
		is_wispnet: bool,
	},
	WsProxy {
		path: String,
		udp: bool,
	},
}

async fn ws_upgrade<F, R>(
	mut req: Request<Incoming>,
	stats_endpoint: Option<String>,
	callback: F,
) -> anyhow::Result<Response<Body>>
where
	F: FnOnce(OnUpgrade, HttpUpgradeResult, Option<String>) -> R + Send + 'static,
	R: Future<Output = anyhow::Result<()>> + Send,
{
	let is_upgrade = is_upgrade_request(&req);

	if !is_upgrade {
		if let Some(stats_endpoint) = stats_endpoint {
			if req.uri().path() == stats_endpoint {
				return send_stats().await;
			}
		}

		debug!("sent non_ws_response to http client");
		return non_ws_resp(&req);
	}

	trace!("recieved request {:?}", req);

	let (resp, fut) = upgrade(&mut req)?;
	// replace body of Empty<Bytes> with Full<Bytes>
	let mut resp = Response::from_parts(resp.into_parts().0, Body::new(Bytes::new()));

	let headers = req.headers();
	let ip_header = if CONFIG.server.use_real_ip_headers {
		get_header(headers, "x-real-ip").or_else(|| get_header(headers, "x-forwarded-for"))
	} else {
		None
	};

	let ws_protocol = headers.get(SEC_WEBSOCKET_PROTOCOL);
	let req_path = req.uri().path().to_string();

	if req_path.ends_with(&(CONFIG.wisp.prefix.clone() + "/")) {
		let has_ws_protocol = ws_protocol.is_some();
		let is_wispnet =
			CONFIG.wisp.has_wispnet() && req.uri().query().unwrap_or_default() == "net";
		tokio::spawn(async move {
			if let Err(err) = (callback)(
				fut,
				HttpUpgradeResult::Wisp {
					has_ws_protocol,
					is_wispnet,
				},
				ip_header,
			)
			.await
			{
				error!("error while serving client: {:?}", err);
			}
		});
		if let Some(protocol) = ws_protocol {
			resp.headers_mut()
				.append(SEC_WEBSOCKET_PROTOCOL, protocol.clone());
		}
	} else if CONFIG.wisp.allow_wsproxy {
		let udp = req.uri().query().unwrap_or_default() == "udp";
		tokio::spawn(async move {
			if let Err(err) = (callback)(
				fut,
				HttpUpgradeResult::WsProxy {
					path: req_path,
					udp,
				},
				ip_header,
			)
			.await
			{
				error!("error while serving client: {:?}", err);
			}
		});
	} else {
		debug!("sent non_ws_response to http client");
		return non_ws_resp(&req);
	}

	Ok(resp)
}

pub async fn route_stats(stream: ServerStream) -> anyhow::Result<()> {
	let stream = TokioIo::new(stream);
	Builder::new()
		.serve_connection(stream, service_fn(move |_| async { send_stats().await }))
		.await?;
	Ok(())
}

pub async fn route(
	stream: ServerStream,
	stats_endpoint: Option<String>,
	callback: impl FnOnce(ServerRouteResult, Option<String>) + Clone + Send + 'static,
) -> anyhow::Result<()> {
	match CONFIG.server.transport {
		SocketTransport::WebSocket => {
			let stream = TokioIo::new(stream);

			Builder::new()
				.serve_connection(
					stream,
					service_fn(move |req| {
						let callback = callback.clone();

						ws_upgrade(
							req,
							stats_endpoint.clone(),
							|fut, res, maybe_ip| async move {
								let ws = fut.await.context("failed to await upgrade future")?;

								match res {
									HttpUpgradeResult::Wisp {
										has_ws_protocol,
										is_wispnet,
									} => {
										let ws = ws.downcast::<TokioIo<ServerStream>>().unwrap();
										let ws =
											chain(Cursor::new(ws.read_buf), ws.io.into_inner());

										let ws = tokio_websockets::ServerBuilder::new()
											.limits(Limits::default().max_payload_len(Some(
												CONFIG.server.max_message_size,
											)))
											.serve(ws);
										let (read, write) =
											TokioWebsocketsTransport(ws).split_fast();

										let result = if is_wispnet {
											ServerRouteResult::Wispnet {
												stream: (Either::Left(read), Either::Left(write)),
											}
										} else {
											ServerRouteResult::Wisp {
												stream: (Either::Left(read), Either::Left(write)),
												has_ws_protocol,
											}
										};

										(callback)(result, maybe_ip);
									}
									HttpUpgradeResult::WsProxy { path, udp } => {
										let ws = tokio_websockets::ServerBuilder::new()
											.limits(Limits::default().max_payload_len(Some(
												CONFIG.server.max_message_size,
											)))
											.serve(TokioIo::new(ws));
										let ws = WebSocketStreamWrapper(ws);
										(callback)(
											ServerRouteResult::WsProxy {
												stream: ws,
												path,
												udp,
											},
											maybe_ip,
										);
									}
								}

								Ok(())
							},
						)
					}),
				)
				.with_upgrades()
				.await?;
		}
		SocketTransport::LengthDelimitedLe => {
			let codec = LengthDelimitedCodec::builder()
				.little_endian()
				.max_frame_length(usize::MAX)
				.new_codec();

			let (read, write) = stream.split();
			let read = MapErr(FramedRead::new(read, codec.clone()));
			let write = MapErr(FramedWrite::new(write, codec));

			(callback)(
				ServerRouteResult::Wisp {
					stream: (Either::Right(read), Either::Right(write)),
					has_ws_protocol: true,
				},
				None,
			);
		}
	}
	Ok(())
}
