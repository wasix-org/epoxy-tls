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
  --env WISP_AUTOCONFIGURE_DOMAIN=https://app.example.com/wisp-autoconfigure
```

Available variables are:

- `WISP_SERVER_BIND`, `WISP_SERVER_RUNTIME`, `WISP_SERVER_TRANSPORT`,
  `WISP_SERVER_LOG_LEVEL`, `WISP_AUTOCONFIGURE_DOMAIN`
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
different origin. `WISP_AUTOCONFIGURE_DOMAIN` therefore identifies a bridge
page you control, such as `https://app.example.com/wisp-autoconfigure`. The
WISP landing page loads it in a hidden iframe and sends this message:

```js
{
  type: "wisp-autoconfigure",
  version: 1,
  endpoint: "wss://wisp.example.com/"
}
```

The bridge can relay it to already-open tabs on its own origin:

```js
const channel = new BroadcastChannel("wisp-autoconfigure");

window.addEventListener("message", (event) => {
  if (event.origin !== "https://wisp.example.com") return;
  if (event.data?.type !== "wisp-autoconfigure" || event.data.version !== 1) return;
  channel.postMessage(event.data);
});
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

The bridge response must permit the WISP origin through its CSP
`frame-ancestors` directive and must not send `X-Frame-Options: DENY`.

The packaged policy only permits TCP ports 80 and 443 and blocks direct,
loopback, multicast, and non-global targets. Add WISP authentication or enforce
access at the deployment edge before exposing it publicly; an unauthenticated
WISP endpoint is still an outbound proxy.
