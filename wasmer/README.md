# Epoxy WISP proxy for Wasmer

This directory is a ready-to-run Wasmer package for the WASIX Epoxy server.
Its package manifest mounts `config/config.toml` at `/etc/epoxy/config.toml`
and passes that path to the `wisp-proxy` entrypoint as a WASI argument.

Build the module from the repository root:

```console
./wasmer/build.sh
```

Then run the package:

```console
wasmer run ./wasmer --net
```

The configured server listens at `0.0.0.0:4000`. Ordinary requests to `/`
show the connection page; WebSocket upgrades on the same URL enter WISP.

Individual settings can still be overridden with the supported `WISP_*`
environment variables. For example:

```console
wasmer run ./wasmer --net \
  --env WISP_AUTOCONFIGURE_DOMAIN=https://app.example.com/wisp-autoconfigure
```
