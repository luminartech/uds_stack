# uds_on_ip

`uds_on_ip` owns **ISO 14229-5 (`UDSonIP`)**: the application profile that
binds Unified Diagnostic Services to a `DoIP` transport. It owns two layers of
that standard — clause 8, the application profile (the `A_PDU` format, TCP
connection handling around `DiagnosticSessionControl` and `ECUReset`, and which
timing parameters apply), and clause 11, the mapping of `T_PDU` service
primitives onto `DoIP`'s.

## Where this fits

```text
  consuming application
       ↕  typed service traits / typed client calls
  uds_services      the driver — owns Client/Server, declares UdsTransport
       ↓  UdsTransport
  uds_on_ip         ISO 14229-5 profile + DoIP mapping  ← this crate
       ↓
  simple_doip       ISO 13400-2
```

This crate is **wholly below** the session layer in the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace. It hosts no
driver, calls nothing upward, and knows nothing about services. It implements
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s
`UdsTransport` trait, and depends on
[`simple_doip`](https://crates.io/crates/simple_doip) (ISO 13400-2) below it and
on [`uds_protocol`](https://crates.io/crates/uds_protocol) and
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)
for their shared vocabulary. The dependency edge runs from here to
`uds_services`, never the other way — `uds_services` never names a transport,
which is what keeps a binding additive at the application.

[`ARCHITECTURE.md`](ARCHITECTURE.md) has the fuller picture, including the
seams to `simple_doip` and `uds_services` and the standard's own citations.
**Provisional**: it is a design document that predates parts of the current
code and is being replaced by the sphinx-needs set under
[`docs/`](https://github.com/luminartech/uds_stack/tree/main/docs) at the
workspace root; where the two disagree, `docs/` wins, and corrections found
in the meantime are marked in place in the file itself.

## What this crate deliberately does not do

It does not decode UDS messages — that is
[`uds_protocol`](https://crates.io/crates/uds_protocol) — and it does not
dispatch services, choose negative response codes, or know what a data
identifier is; those are ISO 14229-1 behaviour and belong to
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services).
It holds no ISO 14229-2 vocabulary: addressing, the service primitives and the
session state machine are
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)'s,
and are used from there rather than redeclared here.

The one exception is narrow and forced by the standard: clause 8 keys TCP
connection handling on two specific service identifiers
(`DiagnosticSessionControl` and `ECUReset`), so this crate recognises those two
bytes and nothing else about them.

It also names no async runtime. An `async fn` implies neither an executor nor
`std`, but a runtime *dependency* would compromise the `no_std` build, so
`DoIpTransport` is generic over its socket and an adapter for tokio, embassy,
or a bare-metal driver is additive rather than built in.

## Status

**Alpha (`0.2.0-alpha.1`), not yet published.** The public API shape is
settled — `DoIpTransport<S>` implements `uds_services::UdsTransport` — but the
transport method bodies are still `todo!()`: sending a message
(`t_data_req`), reading the next event (`next_event`), and reading the clock
(`now`) all panic today rather than doing anything. Constructing a transport
and recording its outbound size bound already work and are exercised by unit
tests; the wire-level send/receive path is the work that remains. See the
[workspace README](https://github.com/luminartech/uds_stack#status) for how
this compares to the rest of the stack.

## Feature flags

| Feature | Implies | What it gives you |
|---------|---------|--------------------|
| `std` *(off by default)* | `alloc` | `std::error::Error` for `Error`, and `std` on `simple_doip`/`uds_protocol`. |
| `alloc` | — | The `alloc`-only layers of `simple_doip` and `uds_protocol`. Nothing in this crate's own code needs it. |

`default = []`, so the crate is `no_std` out of the box.

## `no_std` support

`no_std` and allocation-free in its default configuration: no public type
contains a `Vec` or a `String`, and an inbound message borrows the receive
buffer. A bare-metal AURIX `TC4x` target is a qualification target for this
stack, which is why no feature here ever names a runtime.

## Usage

The API shape can be exercised today even though the transport methods
themselves are not implemented yet:

```rust
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_services::UdsTransport;

// `S` is your own socket type; nothing here names a runtime.
let socket = (); // stand-in until a real async or bare-metal socket is wired in
let mut transport = DoIpTransport::new(socket, bench_reloads());

// Learned from the peer's entity status response, once one arrives.
transport.set_outbound_max(Some(4096));
assert_eq!(transport.outbound_max(), Some(4096));
```

`bench_reloads()` gives the conventional `tP6` reload pair for a desk setup —
see its documentation for why it is not a configuration for a vehicle.
Calling `t_data_req`, `next_event`, or `now` on the transport still panics; see
[Status](#status).

## Relationship to the standards

Neither licence below grants any right in ISO 14229-5, which remains ISO's,
and no text of the standard is reproduced here or in `ARCHITECTURE.md`. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate
in this stack.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option.
