# Epoxy WISP proxy for Wasmer

This directory is a ready-to-run Wasmer package for the WASIX Epoxy server.
Its package manifest mounts `config/config.toml` at `/etc/epoxy/config.toml`
and passes that path to the `wisp-proxy` entrypoint as a WASI argument.

Run the published proxy locally:

```console
wasmer run wasmer/wisp-server --net
```

To build and run the package from source instead, use:

```console
./wasmer/build.sh
wasmer run ./wasmer --net
```

The configured server listens at `0.0.0.0:4000`. Ordinary requests to `/`
show the connection page; WebSocket upgrades on the same URL enter WISP.

Individual settings can still be overridden with the supported `WISP_*`
environment variables. For example:

```console
wasmer run wasmer/wisp-server --net \
  --env WISP_AUTOCONFIGURE=https://app.example.com/wisp-autoconfigure
```

Available variables are:

- `WISP_SERVER_BIND`, `WISP_SERVER_RUNTIME`, `WISP_SERVER_TRANSPORT`,
  `WISP_SERVER_LOG_LEVEL`, `WISP_AUTOCONFIGURE`
- `WISP_PROTOCOL_ALLOW_WSPROXY`, `WISP_PROTOCOL_BUFFER_SIZE`,
  `WISP_PROTOCOL_PREFIX`, `WISP_PROTOCOL_V2`, `WISP_PROTOCOL_EXTENSIONS`
- `WISP_STREAM_TCP_NODELAY`, `WISP_STREAM_BUFFER_SIZE`,
  `WISP_STREAM_ALLOW_UDP`, `WISP_STREAM_ALLOW_WSPROXY_UDP`,
  `WISP_STREAM_ALLOW_DIRECT_IP`, `WISP_STREAM_ALLOW_LOOPBACK`,
  `WISP_STREAM_ALLOW_MULTICAST`, `WISP_STREAM_ALLOW_GLOBAL`,
  `WISP_STREAM_ALLOW_NON_GLOBAL`, `WISP_STREAM_ALLOW_PORTS`

Boolean values accept `true` or `false`; extension and port lists are
comma-separated. Ports accept either one port (`443`) or an inclusive range
(`8000-9000`). Environment values overlay the packaged configuration.

## Cross-origin browser autoconfiguration

Browsers do not allow one unrelated tab to directly address another tab on a
different origin. `WISP_AUTOCONFIGURE` therefore identifies a bridge
page you control, such as `https://app.example.com/wisp-autoconfigure`. The
WISP landing page shows an **Automatically connect to …** link. Clicking it
navigates the tab to that URL with the WISP endpoint in the `endpoint` query
parameter. Requiring a click keeps the landing page stable for screenshots and
previews. Making the bridge top-level is important because modern browsers
partition cross-tab channels used by third-party iframes.

The bridge can relay the query parameter to already-open tabs on its own origin:

```js
const channel = new BroadcastChannel("wisp-autoconfigure");
const endpoint = new URL(location.href).searchParams.get("endpoint");
channel.postMessage({ type: "wisp-autoconfigure", version: 1, endpoint });
```

The existing application tab listens on the same channel:

```js
const channel = new BroadcastChannel("wisp-autoconfigure");
channel.addEventListener("message", ({ data }) => {
  if (data?.type === "wisp-autoconfigure" && data.version === 1) {
    configureWisp(data.endpoint);
  }
});
```

The packaged policy only permits TCP ports 80 and 443 and blocks loopback,
multicast, and non-global targets. It permits direct global IP addresses because
browser WASIX clients connect to the address selected by their DNS resolver.
Add WISP authentication or enforce access at the deployment edge before exposing
it publicly; an unauthenticated WISP endpoint is still an outbound proxy.
