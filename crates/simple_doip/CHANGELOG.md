# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

This file was reconstructed from the commit and pull-request history when the
crate was prepared for publication, so entries before that point describe what
changed rather than what was announced at the time.

## [Unreleased]

0.7.0 replaces the async client, the async server and the bare-metal entity with a
`no_std`, allocation-free tester and entity behind the new `connection` feature, and
reshapes several protocol-core messages to match ISO 13400-2:2019.
[MIGRATING.md](MIGRATING.md) maps each removed type to its replacement, with an
example for each.

### Removed

- **Breaking:** the `bare_metal_entity` module: `Entity`, `EntityConfig`,
  `Callbacks`, `TcpVerdict` and the constants `VIN_LEN`, `EID_LEN`, `GID_LEN`,
  `UDS_RESP_CAP`, `TCP_RX_CAP` and `MAX_RX_PAYLOAD`. It was not behind a feature, so
  this breaks every user of the crate, default features included. Use
  `entity::Entity` on an `edge-nal` backend instead, with `Entity::with_discovery`
  for the UDP side; see [MIGRATING.md](MIGRATING.md).
- **Breaking:** the `client` module (`Client`, `ClientOptions`,
  `RoutingActivationOptions`, `AddressType`) and the `connection` module's
  `Connector` and `ConnectorSocket`. Use `tester::Tester`.
- **Breaking:** the `server` module (`Server`, `ServerConnectionHandler`,
  `ResponseWriter`, `ClientConnectionInfo`). Use `entity::Entity`.
- **Breaking:** the `client` and `server` features. A manifest that enables either
  no longer resolves; enable `connection` for the replacements.
- **Breaking:** `simple_doip::Error`, which only the client and server returned.
- **Breaking:** `impl Default for OwnedMessage`. It built a placeholder diagnostic
  message for the old client; build the message you mean instead.
- **Breaking:** `TESTER_LOGICAL_ADDRESS`. Its value, `0xE400`, is the first
  functional group address of Table 13, not a tester address. Supply your
  deployment's tester address, from `0x0E00`–`0x0FFF`.
- **Breaking:** `DiagnosticAckCode::is_positive_ack` and `is_negative_ack`. Whether
  an acknowledgement is negative is now its type; see Changed.
- The `async-trait`, `futures` and `embedded-io` dependencies. The first two served
  only the client and server; for `embedded-io`, see Changed.
- The `simple_client` and `echo_server` examples, with the types they showed.

### Added

- The `connection` feature: a `no_std`, allocation-free tester and entity over
  `edge-nal` 0.7, keeping time with `embassy-time` 0.5. It adds `edge-nal`,
  `embedded-io-async`, `embassy-time` and `embassy-futures` as optional dependencies
  and needs none of the crate's other features. The integrator supplies the
  `edge-nal` backend and an `embassy-time` driver and timer queue; the crate
  documentation lists what each must guarantee for the event methods to be
  cancel-safe.
- `service`, available without any feature: ISO 13400-2's connection service with no
  I/O, for a layer above (such as `uds_on_ip`) to be generic over.
  - `DiagnosticConnection`, one connection's `DoIP_Data` request, confirm and
    indication, and `TesterConnection`, a connection that can also `reconnect`
    (bounded by the caller's deadline, returning `Reconnection`), `close`, and
    report its `address` and last `io_error`.
  - `DiagnosticEntity`, a whole entity: events are tagged with the `ConnectionId`
    they arrived on, a request is routed by target address, and `close` is the close
    ISO 14229-5 prescribes after a reset or a session change that leaves the running
    software.
  - `ConnectionEvent` and `EntityEvent`, both exhaustive, so a new event stops the
    build of the layer that must handle it. An indication longer than the caller's
    buffer arrives as `IndicationTruncated`.
  - `DoIpResult`, the twelve `DoIP_Result` values of 8.2.5.
  - `request` is a plain function, not a future: it accepts or refuses the PDU when
    it returns, and the outcome of an accepted one arrives later as exactly one
    `Confirm`. A refusal is a `Refusal` (`EmptyPdu`, `PduTooLarge`, `NoRoom`,
    `NotConnected`), and no confirm follows it.
  - `Timestamp`, a wrapping 32-bit millisecond reading of the implementor's clock,
    which `now` returns and `next_event` takes as its deadline.
  - `TesterAddress`, a logical address in the tester range, and
    `EntityConfig<TESTERS>`, the testers an entity accepts. `EntityConfig` has no
    `Default`: every integrator names its testers.
- `tester::Tester` (`connection`): connects to an entity, activates routing, answers
  alive checks, and is then a `TesterConnection`. One request is outstanding at a
  time; a request with no acknowledgement within `A_DoIP_Diagnostic_Message` is
  confirmed `TimeoutA` and its connection given up, so a late response cannot reach
  a later request. A connection's end is reported once as `Closed`. `reconnect`
  waits `RECONNECT_BACKOFF` (3 s, or `with_reconnect_backoff`) after a loss before
  connecting. Also `ConnectError`, `MIN_N` and `DIAGNOSTIC_MESSAGE_OVERHEAD`.
- `tester::discovery` (`connection`): `identify` broadcasts or directs a vehicle
  identification request (all, by EID, or by VIN) and collects every entity that
  answers within `A_DoIP_Ctrl`; `entity_status` and `power_mode` ask one entity.
- `entity::Entity` (`connection`): an entity's `TCP_DATA` sockets over a bound
  `edge-nal` acceptor, as a `DiagnosticEntity`. It runs routing activation with
  socket arbitration and alive checks, the generic header and diagnostic message
  handlers, and the Table 12 timers inside `next_event`, with `MCTS` connections
  plus the reserve socket. Its memory is fixed by its const parameters. Also
  `EntityAddress`, `AddressError`, `Error` and `ORDERLY_CLOSE_LIMIT`.
- `entity::Entity::with_discovery`: the entity's `UDP_DISCOVERY` socket as well. It
  announces itself, answers vehicle identification (plain, by EID and by VIN),
  entity status and power mode, and NACKs bad headers, all polled by `next_event`.
  The vehicle's identity comes from a `VehicleIdentity`, read each time a frame is
  built; `FixedIdentity` is one for values that do not change. An entity without
  discovery is unchanged and carries no UDP code.
- `Vin`, `EntityId` and `GroupId`, which refuse Table 1's "not set" values
  (`NotSet`), and for a VIN non-ASCII bytes (`VinError`). Decoded messages keep raw
  bytes.
- `messages::DiagnosticMessageNack`, `OwnedDiagnosticMessageNack` and
  `DiagnosticNackCode`, and `Message::diagnostic_message_nack` and
  `OwnedMessage::diagnostic_message_nack`, which stamp payload type `0x8003`.
- `Payload::PowerModeInfoRequest` and `Payload::VehicleIdentificationRequestWithEid`
  and `WithVin` (and their `OwnedPayload` mirrors), which carry the EID or VIN the
  request names.
- `TaType`, with `LogicalAddress::is_functional_group` and `default_ta_type`, which
  classify an address by its Table 13 range. `LogicalAddress` implements `Hash`.
- `wire` re-exports `Sink`, `SliceSink` and `WriteError`.
- The Table 12 parameters `A_DOIP_CTRL`, `A_DOIP_ANNOUNCE_WAIT_MAX`,
  `A_DOIP_ANNOUNCE_INTERVAL` and `A_DOIP_ANNOUNCE_NUM`.
- `MIGRATING.md`, a guide from the removed client, server and bare-metal entity.

### Changed

- **Breaking:** `automotive-wire-codec` 0.4, and `embedded-io` is gone.
  `Encode::encode` writes into `&mut impl wire::Sink` instead of an
  `embedded_io::Write`; to encode into a `&mut [u8]`, wrap it in
  `wire::SliceSink::new`. `MessageError::Io` carries a `wire::WriteError` instead of
  an `embedded_io::ErrorKind`. The `alloc` and `std` features no longer enable
  `embedded-io`'s.
- **Breaking:** a negative diagnostic message acknowledgement is its own message, as
  Tables 24 and 26 give the two separate code tables. `DiagnosticAckCode` holds
  Table 24's positive code and `Reserved(u8)`; the rejection reasons move to
  `DiagnosticNackCode`. `Payload::DiagnosticMessageNack` and its `OwnedPayload`
  mirror carry a `DiagnosticMessageNack` instead of nothing.
  `diagnostic_message_ack` no longer takes a code; build a rejection with
  `diagnostic_message_nack`.
- **Breaking:** `EntityStatusResponse::max_data_size` is `Option<u32>` and
  `VehicleIdentificationResponse::vin_gid_sync_status` is
  `Option<VinGidSyncStatus>`, as both fields are optional on the wire. Their
  decoders accept the shorter forms (3 and 32 bytes), and `None` encodes without the
  field.
- **Breaking:** `Payload::decode` keeps what a directed identification request
  names. `0x0002` and `0x0003` decode to `VehicleIdentificationRequestWithEid` and
  `WithVin` rather than `VehicleIdentificationRequest`, and one shorter than its EID
  or VIN is `MessageError::Incomplete`. `0x4003` decodes to `PowerModeInfoRequest`
  where it was `MessageError::UnsupportedPayloadType`.
- **Breaking:** the minimum supported Rust version is 1.91, up from 1.88.
- `simple_doip` is released in lockstep with `uds_protocol`, `uds_session`,
  `uds_services` and `uds_on_ip`: all five share one version, and 0.7.0 is the
  first.
- The crate's repository is now <https://github.com/luminartech/uds_stack>.
- docs.rs, which builds with all features, documents the `connection` API in place
  of the client and server.

### Fixed

- A negative diagnostic acknowledgement was emitted under the positive
  payload type `0x8002` whatever its code, and a received `0x8003` was decoded
  without its body. The separate types above make a header and its code agree.
- **Breaking:** `TCP_TIMEOUT_ALIVE_CHECK` is 500 ms, Table 12's `T_TCP_Alive_Check`,
  where it was 5 s. Code that timed alive checks by it now waits a tenth as long.

## [0.6.0] — 2026-09-10

### Added

- `MessageError::PayloadTooLarge`, for a payload that cannot be described by
  the `u32` `payload_length` field. `MessageError` is `#[non_exhaustive]`, so
  this is not a breaking change.
- `LICENSE-MIT` and `LICENSE-APACHE`. The manifest had declared
  `MIT OR Apache-2.0` without carrying either text.
- `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`.
- `release-plz.toml`. Versioning, the changelog, tags, GitHub releases
  and the crates.io publish are handled by release-plz through the
  org-wide reusable workflow, matching `uds_protocol` and
  `automotive_wire_codec`. There is no release workflow in this repo.

### Fixed

- **Breaking:** `Message::encode` derives the header's `payload_length` from
  the payload instead of writing the field verbatim. A decoded message keeps
  whatever length the wire declared, and `Payload::decode` need not consume
  all of it, so re-encoding a frame that arrived with a mismatched length
  emitted a frame no decoder would accept. Anything that decodes and re-emits
  -- a proxy, a replay tool, a logging fake -- was turning a
  malformed-but-accepted frame into a corrupt one. Nothing stops compiling;
  what changes is the bytes emitted, which is why this is a minor bump and
  not a patch. For such a frame,
  `decode(encode(m)).header.payload_length` is now the payload's real size
  rather than the length it arrived with.
- `MessageError::PayloadLengthTooShort`'s message said "does match" where it
  meant "does not match".
- `ClientConnectionInfo::logical_address` carries the address the tester
  activated routing with, instead of always being `0x0000`. A handler can now
  tell which tester is asking, and the default `alive_check` answers with the
  right source address. It stays `0x0000` before activation and after an
  activation the handler denied.

### Security

- The committed lockfile pinned three versions with RUSTSEC advisories against
  them — `bytes 1.4.0`, `mio 0.8.8` and `tracing-subscriber 0.3.19`. All three
  are refreshed past their patched versions. No manifest requirement changed;
  every one was already permitted.

### Changed

- docs.rs now builds with all features, so the `client`, `server` and `codec`
  API appears in the published documentation. `default = []`, so the default
  build documents only the `no_std` core.
- `release-plz.toml` and `rust-toolchain.toml` are no longer packaged into
  the published crate.

## [0.5.2] — 2026-09-03

### Fixed

- A failed connection reported `Error::SocketNotBound` instead of the
  underlying connect error, so the reason a connection failed was discarded
  and no log level revealed it.

## [0.5.1] — 2026-08-25

### Fixed

- A response already buffered by the client was dropped when the connection
  closed, instead of being delivered to the caller waiting for it.

## [0.5.0] — 2026-08-25

### Changed

- **Breaking:** the diagnostic-message response timeout is configurable on
  `ClientOptions`, and the client now waits `A_DoIP_Diagnostic_Message` for an
  acknowledgement rather than the 50 ms figure ISO 13400-2 places on a DoIP
  *entity*. The old value timed out on any peer that took longer than 50 ms to
  acknowledge.

## [0.4.0] — 2026-08-19

### Changed

- **Breaking:** `ServerConnectionHandler::diagnostic_message` takes
  `&mut dyn ResponseWriter` and returns `Result<(), Error>`. It previously
  returned a single `OwnedMessage`, so a handler could emit either the
  `DiagnosticMessageAck` or the functional response but not both — which is
  what DoIP prescribes, and what a UDS tester waits for.

### Added

- `ResponseWriter`, so a handler can emit an acknowledgement, any number of
  "response pending" messages, and a final answer, awaiting work between them.
- `Server::run_server_with_listener`, for injecting a bound `TcpListener`.
- `Server::run_udp_responder`, answering vehicle-identification requests over
  UDP.

## [0.3.1] — 2026-08-12

### Changed

- A routing-activation denial is returned to the caller as an error instead of
  only being logged.

## [0.3.0] — 2026-07-30

### Changed

- `MessageError` is built on the `automotive-wire-codec` error fragments, which
  makes that crate's error taxonomy part of this crate's public API.

## [0.2.0] — 2026-07-30

### Changed

- **Breaking:** the protocol core is `no_std` and zero-copy. Messages borrow
  from the receive buffer instead of allocating, `alloc` gates the owned
  mirrors, and `std` and the async layers became opt-in Cargo features
  (`default = []`).

## [0.1.0]

Initial implementation: DoIP message types, framing, and an async client and
server over tokio.

<!-- Only v0.1.0, v0.5.1 and v0.5.2 were ever tagged, so the intermediate
     versions have no comparison range to link. -->

[0.1.0]: https://github.com/luminartech/simple_doip/releases/tag/v0.1.0
[0.5.1]: https://github.com/luminartech/simple_doip/compare/v0.1.0...v0.5.1
[0.5.2]: https://github.com/luminartech/simple_doip/compare/v0.5.1...v0.5.2
[0.6.0]: https://github.com/luminartech/simple_doip/compare/v0.5.2...v0.6.0
