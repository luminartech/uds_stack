# simple_doip

`simple_doip` owns ISO 13400-2: Diagnostics over IP (DoIP). It is a `no_std`,
zero-copy protocol core with optional async client and server, and a `no_std`
tester and entity over `edge-nal`.

[`ARCHITECTURE.md`](ARCHITECTURE.md) describes how the crate is put together —
the feature-gated layering, the sans-io framing/decode seam, the error taxonomy,
the relationship to `automotive-wire-codec`, and the known issues and deferred
refactors a new maintainer should read before changing anything.
**Provisional**: it is being replaced by the sphinx-needs set under
[`docs/`](https://github.com/luminartech/uds_stack/tree/main/docs) at the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace root, and
where the two disagree, `docs/` wins.

## Where this fits

This crate is the transport at the bottom of the `uds_stack` workspace: it
knows DoIP framing and nothing about UDS.
[`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
(ISO 14229-5) is the one crate in this stack that depends on it, mapping the
`UDSonIP` application profile onto the DoIP messages this crate provides.

## What this crate deliberately does not do

It models protocol **structure** only. It knows what a diagnostic message
looks like on the wire; it does not know what the UDS bytes inside one *mean*.
`Payload::DiagnosticMessage` carries `user_data: &[u8]` and nothing more — no
service dispatch, no session timing, no UDS decoding. Those are
[`uds_protocol`](https://crates.io/crates/uds_protocol)'s,
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)'s
and
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s
concerns, and this crate has no dependency on any of them.

## Status

Released in lockstep with the rest of the stack, at one shared version; 0.7.0
is not released yet, and 0.6.0 is the last release on crates.io. See the
[workspace README](https://github.com/luminartech/uds_stack#status) for where
the other crates in the stack stand.

## Scope and limitations

The protocol core — framing, message encode/decode — is golden-vector tested
against the on-wire format. The async `client` and `server` cover the common
case, within these bounds:

- **No TLS.** Connections are in the clear on `TCP_PORT` (`13400`);
  `TCP_TLS_PORT` (`3496`) is defined by ISO 13400-2 and unused here.
- **The server serves one TCP connection at a time.** A second client cannot
  connect while the first is being served.
- **No unsolicited UDP vehicle announcement.** A tester learns of an entity only
  by asking, and identification is answered on the UDP path only, by
  `Server::run_udp_responder` on a socket the caller binds and drives.
- **The client requires the peer to acknowledge before it responds.** A
  `DiagnosticMessage` that arrives while the client is waiting for the
  acknowledgement is dropped, so a peer that answers first appears never to
  answer at all.

[`ARCHITECTURE.md`](ARCHITECTURE.md) §7 records these and the rest of the
deferred work.

None of this constrains bare-metal or single-client use. On bare metal, the
`connection` feature's `tester::Tester` and `entity::Entity` are a tester and an
entity that allocate nothing; the crate documentation lists what their integrator
supplies.

## Quickstart

The protocol core needs no allocator and no I/O: frame a byte buffer with
`try_frame`, then decode the payload with `Payload::decode`. This block is the
doctest on `try_frame` in [`src/framer.rs`](src/framer.rs), wrapped in a
`fn main` returning `Result` so it compiles as pasted (the doctest gets the
same `Ok(())` from a hidden line instead). That doctest is the canonical,
CI-tested version; if the two ever drift, trust the doctest.

```rust
use simple_doip::{try_frame, messages::{MessageError, Payload}};

fn main() -> Result<(), MessageError> {
    // A complete DoIP NACK frame: 8-byte header + 1-byte body.
    let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
    assert_eq!(consumed, 9);

    let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
    assert!(matches!(payload, Payload::DoIPNack(_)));
    Ok(())
}
```

## Feature flags

The protocol core is `no_std` and zero-copy by default. Everything that pulls in
`alloc`, `std`, or an async runtime is opt-in via Cargo features:

| Feature  | Enables                                                   | Depends on                        |
|----------|------------------------------------------------------------|------------------------------------|
| `alloc`  | Allocator-backed helpers                                    | —                                   |
| `std`    | `std`-backed I/O and error traits                            | `alloc`                            |
| `codec`  | The tokio-util `Encoder`/`Decoder` for DoIP frames           | `std`, `tokio`, `tokio-util`, `bytes` |
| `client` | The async DoIP client                                        | `codec`, `async-trait`, `futures`  |
| `server` | The async DoIP server                                        | `codec`, `async-trait`, `futures`  |
| `connection` | The `no_std` tester and entity, `tester::Tester` and `entity::Entity`, over `edge-nal` | `edge-nal`, `embedded-io-async`, `embassy-time`, `embassy-futures` |

`default = []`, so bare-metal / embedded targets should build with
`default-features = false` to keep the crate `no_std` with no allocator or
runtime dependencies.

`alloc` is what gates `messages::OwnedMessage` and its owned mirrors of the
borrowed message types — the practical reason to enable it is that you need a
message to outlive the buffer it was decoded from (e.g. to move it across a
queue or task boundary).

For development and testing, enable `client` and `server`:

```sh
cargo test --features client,server
```

## Examples

- **`bare_metal_codec`** — encode, frame, and decode with no allocator and no
  I/O. Runs standalone:

  ```sh
  cargo run --example bare_metal_codec --no-default-features
  ```

- **`echo_server`** and **`simple_client`** — a matched pair, not standalone.
  The server listens on `TCP_PORT` (`13400`); the client dials
  `127.0.0.1:13400`. Run each in its own terminal, server first:

  ```sh
  cargo run --example echo_server --features server    # terminal 1
  cargo run --example simple_client --features client   # terminal 2
  ```

  The client's `ConnectorSocket` refuses any `server_address` whose port is not
  `TCP_PORT` (`13400`), returning `Error::InvalidPort` — pointing a client at a
  non-standard port requires your own `Connector` implementation.

  `echo_server` answers a diagnostic message the way DoIP prescribes: first a
  positive acknowledgement carrying the received bytes back in its
  previous-message-data field, then the echo itself as a separate
  `DiagnosticMessage`. Both are written into the `ResponseWriter` the handler
  is given, which is the shape a real UDS response takes.

  `echo_server` calls `run_server`, so it serves TCP only and does not answer
  UDP discovery probes; see `Server::run_udp_responder` for that half.

## Relationship to `automotive-wire-codec`

The wire-level primitives — byte-level `Decode`/`Encode`, `Incomplete`,
`TrailingBytes` — come from the [`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec)
crate. `simple_doip::wire` re-exports what consumers need so that using this
crate does not require a direct dependency on `automotive-wire-codec`.
Because those re-exported types appear in this crate's public API (e.g. in
`MessageError`'s variants and in every message type's trait impls),
`automotive-wire-codec`'s semver is effectively part of `simple_doip`'s own
semver: a breaking change in that crate is a breaking change here too.

## MSRV

The minimum supported Rust version is **1.88**, the UDS stack's single MSRV,
declared once in the workspace manifest and inherited by all five crates.

## Relationship to the standards

Neither licence below grants any right in ISO 13400-2, which remains ISO's, and
no text of the standard is reproduced here or in `ARCHITECTURE.md`. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate in
this stack.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md). Security reports go through GitHub's
private vulnerability reporting; see [`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.
