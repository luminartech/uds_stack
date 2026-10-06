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
(`DiagnosticSessionControl` and `ECUReset`), so this crate recognises those
identifiers and nothing else about the services.

It also names no async runtime. An `async fn` implies neither an executor nor
`std`, but a runtime *dependency* would compromise the `no_std` build, so
`DoIpTransport` is generic over `simple_doip`'s `DiagnosticEntity` — a trait
with no I/O in it — and the sockets, on whatever runtime, are the entity's.

## Status

**Alpha, not yet published** (it releases in lockstep with the rest of the
stack, at 0.7.0, once the stack's functionality has been verified). The public
API shape is
settled — `DoIpTransport<E: DiagnosticEntity>` implements
`uds_services::UdsTransport` — and the server role is implemented: requests
and responses map onto `DoIP_Data` (ISO 14229-5:2022 REQ 4.3, REQ 4.4), and the
connection is closed after a positive `ECUReset` response (REQ 7.11). The close
REQ 7.9 requires after a session change that disconnects waits on the server
stating which changes do. The clock is `embassy-time`'s, so an integrator
links an `embassy-time` driver. The client role is not built. See the
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

A transport wraps your `DoIP` entity — anything implementing
`simple_doip::service::DiagnosticEntity` — and is handed to a `uds_server!`
assembly as its `transport`:

```rust
use simple_doip::service::DiagnosticEntity;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;

fn transport_over<E: DiagnosticEntity>(entity: E) -> DoIpTransport<E> {
    DoIpTransport::new(entity, bench_reloads())
}
```

`bench_reloads()` gives the conventional `tP6` reload pair for a desk setup —
see its documentation for why it is not a configuration for a vehicle.

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

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.
