# Wasmer TCP keepalive extension (version 1)

This private WISP v2 extension controls the outbound TCP socket owned by the
proxy. Enable it with `extensions = ["motd", "tcp-keepalive"]` in `[wisp]`, or
`WISP_PROTOCOL_EXTENSIONS=motd,tcp-keepalive`. The Wasmer package enables it;
ordinary server defaults do not. Clients must explicitly negotiate it.

The extension ID is `0xF2`, with the one-byte metadata payload `[1]` in both INFO
packets. Other versions or metadata lengths are rejected. IDs `0xF0` and `0xF1`
are already used by TWisp and Wispnet. This is a private extension, not part of
the standard WISP protocol.

Requests use packet type `0xF2`, replies `0xF3`. Both use the normal WISP header:
packet type `u8`, followed by the target stream ID `u32` little-endian. Stream
zero is not a TCP stream. An unnegotiated request is an invalid WISP packet and
closes the multiplexor; it is never silently accepted.

The request payload is exactly 10 bytes:

| Field | Type | Meaning |
| --- | --- | --- |
| request ID | u32 LE | Client correlation ID, echoed in the reply |
| operation | u8 | 0: set; 1: get |
| option | u8 | 0: enabled; 1: idle seconds; 2: interval seconds; 3: probe count |
| value | u32 LE | Setter value; ignored by get |

Enabled accepts only 0 or 1. Timing/count setters accept positive values no
larger than `INT_MAX`; the host OS can impose additional limits. Changing an
individual timing option preserves the other timings and the enabled state.

The reply payload is exactly 9 bytes:

| Field | Type | Meaning |
| --- | --- | --- |
| request ID | u32 LE | Request being acknowledged |
| status | u8 | 0: success; 1: invalid; 2: unsupported; 3: bad/closed stream; 4: OS I/O error |
| value | u32 LE | Actual socket value on success, otherwise zero |

Both SET acknowledgements and GET replies read back the socket with
`getsockopt` (or the corresponding WASIX import). Enabled is normalized to 0/1,
since some kernels return their socket option bitmask. Native error codes are
mapped to the statuses above; errors are never converted to successful ACKs.
A setter may have taken effect even if its subsequent readback fails, as with
other nontransactional socket APIs.

A request for a stream that is still resolving/connecting waits for that
connection. The stream is registered when CONNECT is processed, before its
connection task starts. Failure or closure wakes the waiting request with
status 3. UDP and Wispnet forwarding streams return status 2. A duplicated,
owned descriptor keeps the socket identity stable during a request and is
retired when forwarding ends. Registrations retain stream identity even when a
client reuses a closed stream ID. Requests are processed in wire order, and replies
are sent only after the operation has completed. The transport write lock is
not held while waiting for a connection, so failure/close processing can run.

Native builds use POSIX socket options on Linux, Android, macOS, iOS and FreeBSD.
Other native platforms return unsupported for timing options they cannot map.
The WASIX build calls `wasix_32v1.sock_set_opt_size` / `sock_get_opt_size` with
option IDs 27 (idle), 28 (interval), and 29 (count), and the flag imports with
option 12 for enabled. These direct imports avoid the old libc TCP_KEEP* mapping,
which returns ENOSYS. WASIX descriptor duplication uses the existing `fd_dup2`
import because Rust's WASI descriptor-cloning implementation is unsupported.
**The deployed proxy's Wasmer runtime also needs support
for these new option IDs and real socket operations.** Updating only the browser
SDK or the proxy package cannot add support to an older deployment host.

## Validation

`./wasmer/test-native-keepalive.sh` runs the focused native tests against the
source tree. It creates an ignored native workspace because the committed lock
file selects WASIX-only crate versions unavailable on crates.io. The tests
cover real kernel readback, enabled-state preservation, real OS errors,
negotiation/version rejection, requests before connection completion, failed
connections without deadlock, reused stream IDs, and both repeated requests and ordinary data after an
extension packet.

`./wasmer/build.sh` builds the WASIX package module. Building it does not publish
the package or update any deployed proxy.
