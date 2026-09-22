# `uds_protocol`

`uds_protocol` owns ISO 14229-1's message format: encoding and decoding UDS
requests and responses. It targets embedded ECU diagnostics and desktop
tooling alike — `no_std` and allocation-free at its core, with no required
allocator and no async runtime.

[![Crates.io](https://img.shields.io/crates/v/uds_protocol.svg?style=for-the-badge)](https://crates.io/crates/uds_protocol)
[![Docs.rs](https://img.shields.io/docsrs/uds_protocol?style=for-the-badge)](https://docs.rs/uds_protocol)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](./LICENSE-MIT)
[![APACHE License](https://img.shields.io/badge/license-APACHE-blue.svg?style=for-the-badge)](./LICENSE-APACHE)

This library is based on the ISO 14229-1:2020 standard.

## Where this fits

`uds_protocol` is the bottom of the stack described in the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace README: the
message format that everything above it builds on. It has no dependency on any
other crate in the stack.

- [`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)
  (ISO 14229-2) sits beside it in the stack but does not depend on it — the
  session layer never reads a message's content.
- [`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)
  (ISO 14229-1's behaviour, to this crate's format) and
  [`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
  (ISO 14229-5) both depend on this crate directly: they dispatch and transport
  the message types this crate encodes and decodes.

## What this crate does not do

It is a synchronous, allocation-free codec, and nothing more. It owns no
sockets, buffers, or async runtime; it does not dispatch a request to a
handler, choose a negative response code, or hold any session state. Driving
the I/O loop, over `DoIP`, `UDSonIP`, ISO-TP, or anything else, is a caller's
job — see [`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)
for the layer that owns dispatch.

| Service Name                       | Request SID | Response SID | Support |
| ----------------------------------- | ----------- | ------------ | ------- |
| `DiagnosticSessionControl`         | 0x10        | 0x50         | ✓       |
| `EcuReset`                         | 0x11        | 0x51         | ✓       |
| `ClearDiagnosticInformation`       | 0x14        | 0x54         | ✓       |
| `ReadDTCInformation`               | 0x19        | 0x59         | Partial |
| `ReadDataByIdentifier`             | 0x22        | 0x62         | ✓       |
| `ReadMemoryByAddress`              | 0x23        | 0x63         |         |
| `ReadScalingDataByIdentifier`      | 0x24        | 0x64         |         |
| `SecurityAccess`                   | 0x27        | 0x67         | ✓       |
| `CommunicationControl`             | 0x28        | 0x68         | ✓       |
| `Authentication`                   | 0x29        | 0x69         |         |
| `ReadDataByPeriodicIdentifier`     | 0x2A        | 0x6A         |         |
| `DynamicallyDefineDataIdentifier`  | 0x2C        | 0x6C         |         |
| `WriteDataByIdentifier`            | 0x2E        | 0x6E         | ✓       |
| `InputOutputControlByIdentifier`   | 0x2F        | 0x6F         |         |
| `RoutineControl`                   | 0x31        | 0x71         | ✓       |
| `RequestDownload`                  | 0x34        | 0x74         | ✓       |
| `RequestUpload`                    | 0x35        | 0x75         | ✓       |
| `TransferData`                     | 0x36        | 0x76         | ✓       |
| `RequestTransferExit`              | 0x37        | 0x77         | ✓       |
| `RequestFileTransfer`              | 0x38        | 0x78         | ✓       |
| `WriteMemoryByAddress`             | 0x3D        | 0x7D         |         |
| `TesterPresent`                    | 0x3E        | 0x7E         | ✓       |
| `AccessTimingParameters`[^1]       | 0x83        | 0xC3         |         |
| `SecuredDataTransmission`          | 0x84        | 0xC4         |         |
| `ControlDtcSetting`                | 0x85        | 0xC5         | ✓       |
| `ResponseOnEvent`                  | 0x86        | 0xC6         |         |
| `LinkControl`                      | 0x87        | 0xC7         |         |

[^1]: `AccessTimingParameters` (0x83) was defined in ISO 14229-1:2013 and removed in the 2020
edition. `UdsServiceType` still names it so a 2013-era service byte round-trips rather than
becoming an unrecognized `Other`, but it is not part of the standard this crate targets.

## `no_std` support

The crate is `no_std` and allocation-free at its core; everything below is
additive. Encoding and decoding work with borrowed slices alone — no service in
the table above needs an allocator to be represented.

## Features

Default is `std`.

| Feature | Implies | What it gives you |
|---------|---------|-------------------|
| `std` *(default)* | `alloc` | `std::error::Error` for [`Error`]. Turn it off for bare metal. |
| `alloc` | — | The `alloc`-only conveniences. Nothing in the wire codec needs it; encoding and decoding work with borrowed slices alone. |
| `serde` | — | `Serialize`/`Deserialize` on the request, response and parameter types. The only optional integration usable on a bare-metal target: it is wired as a core-only dependency and picks up serde's `alloc`/`std` layers only when this crate's own are on. |
| `utoipa` | `std`, `serde` | `ToSchema` for `OpenAPI` generation. |
| `clap` | `std` | `ValueEnum` on the sub-function enums, for building a CLI tester. |

Two implications are worth knowing about:

- **`utoipa` implies `serde`** because a `ToSchema` here describes the *`serde`* representation.
  Several types serialize as a single protocol byte rather than as their Rust shape — a
  `DataFormatIdentifier` is `33`, not `{"compression_method":2,"encryption_method":1}` — and the
  schemas are written to match. Without `serde` the schema would describe a wire format that
  build cannot produce.
- **`utoipa` and `clap` imply `std`** because their derive macros expand to `std::`, `String` and
  `Vec` paths inside this crate, which cannot compile under `#![no_std]`.

With `serde` enabled, the types that carry a range invariant deserialize through the same
classifier the wire decoder uses, so a hand-written payload cannot construct a value that would
encode to an illegal byte.

## Usage

`uds_protocol` is a synchronous, allocation-free codec. It owns no sockets, buffers, or
async runtime. To use it over any transport (`DoIP`, `UDSonIP`, ISO-TP, …):

- **Decode** an inbound frame from the `&[u8]` you received.
- **Encode** an outbound frame into any `automotive_wire_codec::Sink` (or a caller-owned
  buffer sized with `encoded_size()`, via `SliceSink`).

Drive the I/O loop from your own sync or async layer — the crate never blocks or awaits.

### Encode (build a request)

```rust
use automotive_wire_codec::SliceSink;
use uds_protocol::{Encode, TesterPresentRequest};

let req = TesterPresentRequest::new(false);
let mut buf = [0u8; 8];
let mut sink = SliceSink::new(&mut buf);
let written = Encode::encode(&req, &mut sink).unwrap();
// `buf[..written]` is the wire frame, ready to hand to your transport.
```

### Decode (parse a response)

```rust
use uds_protocol::{Decode, Response};

// `frame` is the &[u8] your transport handed you.
let frame = [0x7E, 0x00];
let (response, _rest) = Response::decode(&frame).unwrap();
```

The decoded value **borrows** from `frame`: it points into that buffer (like a `struct`
overlaid on a `char buf[]`) and is valid only while `frame` lives. Copy out any fields
you need to keep before the buffer is reused.

## Service coverage

A checkmark in the table above means the service decodes into a typed
[`Request`]/[`Response`] variant; [`NegativeResponse`] is typed as well. Every
other service [`UdsServiceType`] enumerates decodes into [`Request::Other`] /
[`Response::Other`], carrying the service type and the raw payload bytes for
pass-through — an unmodelled service is handed to you intact rather than
rejected.

## Wire codec dependency

`uds_protocol` builds its byte-level decoding on top of the [`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec)
crate, and re-exports its `Incomplete`, `TrailingBytes` and `InvalidWidth` types and its codec
traits (`Encode`, `Decode`, `DecodeIter`) at the crate root. These types are intentionally part of `uds_protocol`'s public API:
they are shared across the Luminar automotive protocol crates so that callers handling multiple
protocols see one consistent short-read/trailing-data error shape. Because of this, a semver-major
release of `automotive-wire-codec` is a breaking change for `uds_protocol` as well.

## Status

Published at 0.1.0. Under active development, and versioned independently of
the rest of the stack — see the
[workspace README](https://github.com/luminartech/uds_stack#status) for where
the other crates stand. The service coverage table above is the honest measure
of what is implemented today; a request without a checkmark decodes as
[`Request::Other`] rather than failing.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## Licence

Licensed under either of [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE)
at your option.

Neither licence grants any right in ISO 14229-1 itself, which remains ISO's —
see
[Relationship to the standards](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README.
