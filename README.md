# epoxy
Epoxy is an encrypted proxy for browser javascript.
It allows you to make requests that bypass CORS without compromising security by running SSL/TLS inside webassembly and using the [Wisp protocol](https://github.com/mercuryworkshop/wisp-protocol/) to proxy TCP connections.

It also has a Wisp library implementation for Rust and a performant Wisp server built in Rust.

See the [client readme](client/README.md) and [server readme](server/README.md) for instructions on how to build and use them.

## Wasmer and WASIX

The [`wasmer`](wasmer) directory contains a ready-to-run Wasmer package for
the Epoxy WISP server. It includes the WASIX build script, packaged module,
mounted server configuration, and command entrypoint. See
[`wasmer/README.md`](wasmer/README.md) for build and run instructions.
