# simple_doip

`simple_doip` owns ISO 13400-2:2019: Diagnostics over IP (DoIP). It is a
`no_std`, zero-copy protocol core, and a `no_std` tester and entity over
`edge-nal`. In its documentation, a table, figure, clause or requirement cited
with no document named is ISO 13400-2:2019's; any other document is named where
it is cited.

[`ARCHITECTURE.md`](ARCHITECTURE.md) describes how the crate is put together:
the feature-gated layering, the sans-io framing and decode seam, the error
taxonomy, the relationship to `automotive-wire-codec`, and the known issues and
deferred work. **Provisional**: the sphinx-needs set under
[`docs/`](https://github.com/luminartech/uds_stack/tree/main/docs) at the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace root will
take it in, and where the two disagree, `docs/` wins.

## Where this fits

This crate is the transport at the bottom of the `uds_stack` workspace: it
knows DoIP framing and nothing about UDS.
[`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
(ISO 14229-5) is the one crate in this stack that depends on it, mapping the
`UDSonIP` application profile onto the DoIP messages this crate provides.

## Scope

It models protocol **structure**: what a diagnostic message looks like on the
wire. The UDS bytes inside one are opaque to it, and
`Payload::DiagnosticMessage` carries them as `user_data: &[u8]`. Decoding them
is [`uds_protocol`](https://crates.io/crates/uds_protocol)'s, session timing is
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)'s,
and service dispatch is
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s;
this crate depends on none of them.

## Status

Released in lockstep with the rest of the stack, at one shared version.
[`MIGRATING.md`](MIGRATING.md) maps the async client, the async server and
`bare_metal_entity` of 0.6 to what 0.7 has in their place. See the
[workspace README](https://github.com/luminartech/uds_stack#status) for where
the other crates in the stack stand.

## Scope and limitations

The protocol core, framing and message encode/decode, is golden-vector tested
against the on-wire format. The `connection` feature's `tester::Tester` and
`entity::Entity` are a tester and an entity that allocate nothing; the crate
documentation lists what their integrator supplies. The entity announces itself
and answers vehicle identification, entity status and power mode over UDP, and
`tester::discovery` finds entities and asks them the same. Within these bounds:

- **No TLS.** Connections are in the clear on `TCP_PORT` (`13400`);
  `TCP_TLS_PORT` (`3496`) is defined by ISO 13400-2 and unused here.
- **A tester carries one unconfirmed request at a time.** A second request
  before the first is confirmed is refused.

[`ARCHITECTURE.md`](ARCHITECTURE.md) §7 records these and the rest of the
deferred work.

## Quickstart

The protocol core needs no allocator and no I/O: frame a byte buffer with
`try_frame`, then decode the payload with `Payload::decode`, as the doctest on
`try_frame` in [`src/framer.rs`](src/framer.rs) does. Here it is wrapped in a
`fn main` returning `Result`, so it compiles as pasted.

```rust
use simple_doip::{try_frame, messages::{MessageError, Payload}};

fn main() -> Result<(), MessageError> {
    // A complete DoIP NACK frame: 8-byte header + 1-byte body.
    let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
    assert_eq!(consumed, 9);

    let payload = Payload::decode(frame.payload, frame.header.payload_type())?;
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
| `std`    | `MessageError::Std` and its conversion from `std::io::Error` | `alloc`                            |
| `codec`  | The tokio-util `Encoder`/`Decoder` for DoIP frames           | `std`, `tokio-util`, `bytes`, `tracing` |
| `connection` | The `no_std` tester and entity, `tester::Tester` and `entity::Entity`, over `edge-nal` | `edge-nal`, `embedded-io-async`, `embassy-time`, `embassy-futures` |

`default = []`, so bare-metal / embedded targets should build with
`default-features = false` to keep the crate `no_std` with no allocator or
runtime dependencies.

`alloc` gates `messages::OwnedMessage` and the owned mirrors of the borrowed
message types, for a message that outlives the buffer it was decoded from, such
as one moved across a queue or task boundary.

For development and testing, enable every feature:

```sh
cargo test --all-features
```

## Examples

- **`bare_metal_codec`**: encode, frame, and decode with no allocator and no
  I/O. Runs standalone:

  ```sh
  cargo run --example bare_metal_codec --no-default-features
  ```

- **A tester and an entity over loopback** are
  [`tests/entity_std.rs`](tests/entity_std.rs).
- **The whole server path**, a `uds_services` server over `uds_on_ip`'s
  `DoIpTransport` over `Entity`, with `Tester` as the client, is the workspace's
  [`testing/doip-loopback`](https://github.com/luminartech/uds_stack/tree/main/testing/doip-loopback).
- **`Entity` on embassy-net**, for bare metal, is the workspace's
  [`examples/embassy-net-entity`](https://github.com/luminartech/uds_stack/tree/main/examples/embassy-net-entity).

## Relationship to `automotive-wire-codec`

The wire-level primitives, byte-level `Decode`/`Encode`, `Incomplete` and
`TrailingBytes`, come from the [`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec)
crate. `simple_doip::wire` re-exports what consumers need so that using this
crate does not require a direct dependency on `automotive-wire-codec`.
Because those re-exported types appear in this crate's public API, in
`MessageError`'s variants and in every message type's trait impls,
`automotive-wire-codec`'s semver is part of `simple_doip`'s own semver: a
breaking change in that crate is a breaking change here too.

## MSRV

The minimum supported Rust version is **1.91**, the UDS stack's single MSRV,
declared once in the workspace manifest and inherited by every crate in it.

## Relationship to the standards

Neither license below grants any right in ISO 13400-2, which remains ISO's, and
no text of the standard is reproduced here or in `ARCHITECTURE.md`. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate in
this stack.

## Contributing

Pull requests, bug reports and questions are welcome; see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md). Security reports go through GitHub's
private vulnerability reporting; see [`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.
