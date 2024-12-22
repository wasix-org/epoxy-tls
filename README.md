# epoxy
Epoxy is an encrypted proxy for browser javascript.
It allows you to make requests that bypass CORS without compromising security by running SSL/TLS inside webassembly and using the [Wisp protocol](https://github.com/mercuryworkshop/wisp-protocol/) to proxy TCP connections.

It also has a Wisp library implementation for Rust and a performant Wisp server built in Rust.

See the [client readme](client/README.md) and [server readme](server/README.md) for instructions on how to build and use them.
