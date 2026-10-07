# Architecture

This document describes `simple_doip` **as it is today** — the shape of the crate,
why it is shaped that way, and the known rough edges a new maintainer should be
aware of before changing anything. It is written for someone who has never seen
the code.

> **Provisional.** This is a working document, not the crate's formal
> architecture record. It will be superseded by the sphinx-needs architecture
> set under `docs/architecture/`; until that exists, this file is what there is.

For usage, feature flags, and the current gap list, see [`README.md`](README.md).

> **A note on spec citations.** Cite a section, clause, table or requirement of
> ISO 13400-2 only after reading it in the standard's text, and cite the edition
> (`ISO 13400-2:2019 Table 11`). An earlier review of this crate found and removed
> a batch of fabricated spec locators, written when no reader could check one; the
> rule exists so that never recurs. A locator nobody has read is worse than none —
> "per ISO 13400-2" without one is fine. Where a claim rests on a figure, say so,
> and do not paraphrase a figure that has not been read.

---

## 1. What this crate is

`simple_doip` implements Diagnostics over IP (DoIP), the vehicle-diagnostics
transport specified in ISO 13400-2. A DoIP frame is an 8-byte generic header
(protocol version, its bitwise inverse, a 16-bit payload type, a 32-bit payload
length) followed by a payload whose structure is determined by the payload type.

The crate models protocol **structure** only. It knows what a diagnostic message
looks like on the wire; it does not know what the UDS bytes inside one *mean*.
That boundary is deliberate and load-bearing: the semantics of a diagnostic
identifier are application/ECU configuration, not a transport-library concern.
`Payload::DiagnosticMessage` carries `user_data: &[u8]` and nothing more.

The forcing function for the current design is a next-generation sensor that must
act as a diagnostic server/ECU on **strict no-alloc bare metal** — no allocator,
no `Vec`, no owned forms, everything borrows plus fixed buffers. Every layering
decision below follows from that.

---

## 2. Layering

Capability is added in Cargo-feature tiers, each building on the previous.
`default = []`, so the bare crate is the `no_std` core, running up through
owned/alloc mirrors, `std` I/O and errors, the tokio-util codec, and finally the
async client/server. `connection` stands apart: it builds on the core alone, and
is `no_std` with no allocator:

| Tier | Cargo feature | What it adds | Key files |
|---|---|---|---|
| L0 (external) | — | byte-level `Decode`/`Encode` traits, `Incomplete`, `TrailingBytes`, `take` | `automotive-wire-codec` crate |
| Borrowed core | *(none)* | `Message<'a>`, `Payload<'a>`, `Header`, `try_frame` | `src/messages/`, `src/framer.rs` |
| Owned mirror | `alloc` | `OwnedMessage`, `OwnedPayload`, `OwnedDiagnosticMessage`, `OwnedDiagnosticMessageAck`, `OwnedDiagnosticMessageNack` | `src/messages/mod.rs`, `src/messages/payload.rs` |
| std | `std` | `std::io::Error` interop (`MessageError::Std`), `std`-backed error traits | `src/messages/message_error.rs` |
| Codec | `codec` | `MessageCodec`, a `tokio_util::codec` `Encoder`/`Decoder` | `src/message_codec.rs` |
| Async client | `client` | `Client`, `Connector` (trait + `ConnectorSocket`) | `src/client.rs`, `src/client_inner.rs`, `src/socket_manager.rs`, `src/connection.rs` |
| Async server | `server` | `Server`, `ServerConnectionHandler` | `src/server.rs` |
| Connection service | `connection` | `tester::Tester`, a `TesterConnection` (a `DiagnosticConnection` that can reconnect and close) over `edge-nal` and `embassy-time` | `src/tester.rs`, `src/tester/` |

`client` and `server` are each `["codec", ...]` in `Cargo.toml`, so either one
pulls in `codec` (and transitively `std`/`alloc`), but they do **not** pull in
each other: `client_inner.rs`, `socket_manager.rs`, `connection.rs`, and
`Connector` are gated on `client` only, so a `server`-only build gets none of
them. `src/error.rs` (the `Error` type) is gated on `client` **or** `server`,
so it is present in either build. Authorities: `Cargo.toml`'s `[features]`
table and the `#[cfg(feature = ...)]` gating in `src/lib.rs`.

### 2.1 Why borrowed-first

The core type is `Message<'a>`, which borrows its payload bytes directly out of
the receive buffer, so nothing in the default build allocates. The owned form
is a **separate type in a higher tier** (`OwnedMessage`, gated on `alloc`)
rather than a generic instantiation of the core — this costs a small amount of
duplication (`OwnedMessage` mirrors `Message`'s constructors) but keeps the
bare-metal build free of allocator-shaped generic machinery.

The two directions of conversion are `Message::to_owned_message()` and
`OwnedMessage::as_ref()`. `Encode for OwnedMessage` delegates through `as_ref()`
so there is exactly **one** wire implementation; there is no second serializer
that could drift. The borrowed↔owned round trip for every `Payload` variant is
locked by `payload_conversion_roundtrip_all_variants` in `src/messages/mod.rs`.

---

## 3. The sans-io seam

The most important structural decision in the crate is that **framing and payload
interpretation are two separate steps**.

```rust
// step 1 — framing only. Never looks at the payload bytes.
let (frame, consumed) = try_frame(buf)?.expect("complete frame present");

// step 2 — interpretation, invoked by the caller, on the caller's terms.
let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
```

`try_frame` (in `src/framer.rs`) validates the 8-byte header's protocol version
inverse, checks the declared `payload_length` only against how many bytes are
actually available in the buffer (never against what the payload type requires),
delimits one frame, and returns `RawFrame<'a> { header, payload: &'a [u8] }`
plus a consumed byte count. It owns no I/O resource and never interprets the
payload type beyond carrying it in the header. `Ok(None)` means "need more
bytes" — the stream-reassembly contract that lets the same function serve a TCP
socket, an `embedded-io` byte source, or a slice in a test.

Two properties fall out of this split:

1. **A caller can apply its own policy.** When `Payload::decode` fails, the
   caller still holds the header, the raw payload bytes, and the consumed
   count, and can NACK, ignore, log, or skip and resync.
2. **DoIP never decides for the caller.** An unrecognised payload type is
   reported as a typed `Err(MessageError::UnsupportedPayloadType(..))` rather
   than passed through as an opaque variant, but *error ≠ fatal*: whether to
   NACK, drop the connection, or skip the frame is a policy question the
   library refuses to answer.

`frame_then_decode_recoverability` in `src/framer.rs` pins both halves: a valid
frame decodes, and a frame with a valid header but an unmodeled payload type
still frames successfully with `frame.payload` and `consumed` intact.
`huge_payload_length_does_not_overflow` pins that `try_frame` guards against a
hostile `payload_length` of `u32::MAX` by comparing available bytes rather than
computing `Header::SIZE + payload_len`.

`Message::decode` (`impl Decode<'a> for Message<'a>` in `src/messages/mod.rs`)
is the fused convenience path — header + payload in one call — for callers that
have a complete message in hand and do not need the seam. The framing-plus-decode
path is the one the codec and the bare-metal example use.

---

## 4. Error taxonomy

There are **two** error types, and they are not tiers of each other:

- **`MessageError`** (`src/messages/message_error.rs`) — wire-level encode/decode
  failures from the `no_std` core. Available in every build.
- **`Error`** (`src/error.rs`) — client/server transport and session failures:
  socket I/O, timeouts, routing activation refused, invalid logical address,
  unexpected message type for the current protocol state. Gated on
  `client`/`server`. It wraps `MessageError` via a `From` impl.

`MessageError`'s variants fall along **two orthogonal axes**, documented in the
module header of `src/messages/message_error.rs`:

| Variant | Framing tier | Layer |
|---|---|---|
| `VersionInverseIncorrect` | framing-fatal | wire decode |
| `Incomplete` | framing-fatal | wire decode |
| `TrailingBytes` | recoverable | wire decode |
| `PayloadLengthTooShort`, `UnexpectedPayloadType`, `UnsupportedPayloadType` | recoverable (body) | wire decode |
| `Io(embedded_io::ErrorKind)` | recoverable | encode / `embedded-io` |
| `Std(std::io::Error)` (feature `std`) | not a frame property at all | tokio/codec boundary |

`MessageError::is_framing_fatal()` collapses the first axis into one question:
*has stream sync been lost?* If yes, the header cannot be trusted and the only
safe move is to close the connection. If no, the frame boundary is known and
the caller can NACK, ignore, or skip.

Both `Io` and `Std` exist because they carry different information: the
`no_std` core can only produce a flattened `embedded_io::ErrorKind`, while the
tokio layer has a real `std::io::Error` with an OS code and message that
`src/socket_manager.rs` matches on to decide whether a read error means
"connection reset".

`PayloadLengthTooShort` and `UnexpectedPayloadType` (on `MessageError`), and
`UnexpectedAckMessage`, `ValueOutOfRange`, `NackReceived`, and
`InvalidClientType` (on `Error`), are documented in-tree as currently
unreachable — part of the public taxonomy but never produced by this crate.

### 4.1 `is_framing_fatal` is consumed by the codec

`MessageCodec::decode` (`src/message_codec.rs`) is the one place the classifier
drives real behavior: a frame that fails to decode is dropped and the codec
loops to the next one when the error is recoverable, but propagated (buffer
left untouched) when `is_framing_fatal()`. So one unsupported payload type does
not tear down a connection; `unsupported_payload_type_is_skipped_not_fatal`
locks that.

### 4.2 `Incomplete` is classified fatal, including inside a complete frame

`try_frame` never returns `Incomplete` itself — a buffer too short for a
complete frame yields `Ok(None)`. So when `Payload::decode` returns
`Incomplete` inside `MessageCodec::decode`, the frame boundary is already known
and stream sync is not actually at risk; only the body was shorter than its
payload type requires. `is_framing_fatal` does not distinguish this from a
genuinely incomplete frame, so the codec tears the connection down either way.
This is deliberate, not a bug: it is documented on the variant and pinned by
`truncated_body_is_fatal_via_classifier` in `src/message_codec.rs`. Splitting
`Incomplete` or making the classifier context-sensitive would change tested
wire-facing behavior and is left to a deliberate future decision.

---

## 5. Relationship to `automotive-wire-codec`

The byte-level primitives are not defined here. They come from the
[`automotive-wire-codec`](https://crates.io/crates/automotive-wire-codec) crate
(pinned at `0.3` in `Cargo.toml`): `Decode<'a>`, `Encode`, the `Incomplete` and
`TrailingBytes` leaf errors, and the `take` slice helper. Both traits carry an
associated `type Error`, which is why `simple_doip` can keep its own rich
`MessageError` (with `PayloadType`-bearing domain variants) instead of being
forced onto a generic structural codec error.

`src/wire.rs` re-exports `Decode`, `Encode`, `Incomplete`, and `TrailingBytes`
as `simple_doip::wire`, so a consumer does not need its own
`automotive-wire-codec` dependency kept in version lockstep.

**Semver coupling:** because those are re-exports of a foreign crate's types
appearing in this crate's public API, `automotive-wire-codec`'s semver is part
of `simple_doip`'s semver — a codec `0.4` is a breaking change to `simple_doip`
even if not one line of `simple_doip` changes. This is documented on
`src/wire.rs` and must be respected at release time.

Separately, `Payload`, `OwnedPayload`, `MessageError`, and `Error` are all
`#[non_exhaustive]`, so downstream `match` expressions on them must already
carry a wildcard arm.

---

## 6. Module map

### `no_std` core

| Path | Role |
|---|---|
| `src/lib.rs` | Crate docs, feature-gated module tree, DoIP ports and timing constants |
| `src/framer.rs` | `RawFrame`, `try_frame` — the sans-io seam (private module; both types re-exported at the crate root as `simple_doip::try_frame` / `simple_doip::RawFrame`) |
| `src/messages/mod.rs` | `Message<'a>`, `OwnedMessage`, constructors, `Decode`/`Encode` impls |
| `src/messages/header.rs` | `Header` (8 bytes, `Header::SIZE`), `PayloadType`, `ProtocolVersion` |
| `src/messages/payload.rs` | `Payload<'a>` and `OwnedPayload`; `Payload::decode` dispatches on `PayloadType` |
| `src/messages/message_error.rs` | `MessageError` and `is_framing_fatal` |
| `src/messages/traits.rs` | Re-export of `Decode`, `Encode`, `take` from the codec crate |
| `src/messages/*.rs` (rest) | One file per concrete payload body (alive check, diagnostic message, routing activation, entity status, power mode, vehicle identification, NACK codes) |
| `src/logical_address.rs` | `LogicalAddress` newtype plus tester-range validation |
| `src/service.rs` | The connection service's vocabulary, with no I/O: `DiagnosticConnection` (with its `MAX_PDU`), `TesterConnection` (adding `reconnect` and `close`), `DiagnosticEntity`, their events, `DoIpResult`, `TesterAddress` |
| `src/wire.rs` | Re-export surface for the codec crate's types |

`PayloadType` is a closed enum with `Reserved(u16)` and
`ReservedVehicleManufacturer(u16)` catch-alls, so an unknown wire value decodes
into a `Reserved` discriminant at the header level and only fails later, at
`Payload::decode`, with `UnsupportedPayloadType`. That ordering is what makes
the seam described in section 3 usable.

### `connection`

| Path | Role |
|---|---|
| `src/tester.rs` | `Tester`: connect, routing activation, `DiagnosticConnection`, the event loop and its reactions |
| `src/tester/tx.rs` | `Outgoing<N>`, the diagnostic message being written, and `Control`, the activation request or alive check response written ahead of it |
| `src/tester/rx.rs` | `RxBuffer<N>`: bytes read and not yet consumed, and the skipping of a frame longer than `N` |
| `src/tester/confirm.rs` | NACK codes to `DoIpResult`, and the caller's deadline on `embassy-time`'s clock |

### std / async layers

| Path | Role |
|---|---|
| `src/message_codec.rs` | `MessageCodec`: `Decoder<Item = OwnedMessage>` and `Encoder<&OwnedMessage>` |
| `src/error.rs` | `Error`, the transport/session error type (`#[non_exhaustive]`) |
| `src/connection.rs` | `Connector` trait and the default `ConnectorSocket` (TCP, 64 KiB socket buffers, refuses any port but 13400) |
| `src/socket_manager.rs` | Owns the spawned socket task; bridges `FramedRead`/`FramedWrite` to two mpsc channels; enforces the general inactivity timeout |
| `src/client_inner.rs` | The client state machine: a `ControlMessage` enum plus a select loop matching responses to pending requests |
| `src/client.rs` | Public `Client<Conn>` — connect, routing activation, send/receive diagnostic messages |
| `src/server.rs` | `Server<T>`, `ServerConnectionHandler`, `ResponseWriter`, `ClientConnectionInfo` |

The client is a channel sandwich, described in one comment at the top of
`src/client_inner.rs`:

```
User → Client → control_sender → Inner → SocketManager.sender  → TCP → Server
User ← Client ← update_receiver ← Inner ← SocketManager.receiver ← TCP ← Server
```

`Client` and `SocketManager` are generic over `Conn: Connector`, so a caller can
substitute its own transport (TLS, a test double, a non-TCP link) without
touching the protocol logic.

#### The pending-request lifecycle

`Inner` (`src/client_inner.rs`) tracks at most one in-flight request in
`active_request: Option<ControlMessage>` (`AwaitAck` or `AwaitResponse`), each
owning a oneshot `Sender` that completes the caller's `await`. Invariant: if a
read of `active_request` finds the wrong variant, it must put the value back,
not drop it — `Option::take` empties the field unconditionally, and discarding
a non-matching value drops its `Sender`, which silently kills the in-flight
request with no error. This was found as a real bug in the routing-activation
path; `negative_ack_during_routing_activation_does_not_drop_pending_request`
and two other regression tests in `tests/integration_test.rs` pin the fix. Any
new code reading `active_request` must preserve the restore-on-mismatch shape.

### Tests and examples

- `tests/golden_vectors.rs` + `tests/golden/` — frozen hex fixtures of the
  exact encoded bytes of every wire type. **Do not regenerate them.** They
  cover bodies, plus whole frames where a constructor chooses the header
  (the diagnostic message acknowledgements' `0x8002`/`0x8003`).
- `tests/integration_test.rs` — real client-against-real-server over loopback TCP.
- `tests/udp_identification.rs` — drives `Server::run_udp_responder` on a
  loopback `UdpSocket`.
- `tests/nested_encode.rs` — permanent regression test for the encode hot path.
- `examples/bare_metal_codec.rs` — encode/frame/decode into a `[u8; N]`, no
  allocator; builds and runs with `--no-default-features` as proof the core is
  genuinely allocator-free.
- `examples/simple_client.rs` / `examples/echo_server.rs` — a matched async pair.

---

## 7. Known issues and deferred work

None of this is scheduled; each is a decision for the crate's next owner.

### 7.1 `is_response` belongs on `PayloadType`, not `Message`

`Message::is_response` (`src/messages/mod.rs`) reads only
`self.header.payload_type` and never touches `self.payload` — it is a pure
`PayloadType → PayloadType` relation wearing a `Message` method signature.
Moving it to `PayloadType` would make the relation testable without
constructing a `Message`, delete `OwnedMessage::is_response` (today a pure
pass-through), and leave the one production call site
(`Inner::process_received_message` in `client_inner.rs`) a one-line change.

### 7.2 Three write-only fields

- **`SocketManager::session_id: u16`** (`src/socket_manager.rs`) — initialised
  to `0`, incremented on every `send`, never read anywhere in the crate. Dead
  state that silently wraps at 65536 sends.
- **`Server::active_connections: AtomicUsize`** (`src/server.rs`) — incremented
  and decremented by an RAII guard on every connection; loaded only by three
  unit tests that pin the guard's RAII behavior, never by production code
  (there is no connection limit or metric consuming it).
- **`Inner::run: bool`** (`src/client_inner.rs`) — assigned in four places;
  the select loop has no `while self.run` check. Shutdown actually happens via
  the control channel closing or `process_received_message` returning `true`.

Each should either be wired to something real or deleted.

### 7.3 Other rough edges

The README's **Scope and limitations** section names these for an integrator
choosing the crate; the mechanics are here:

- No TLS; no unsolicited UDP vehicle announcement at power-on (identification
  requests over UDP *are* answered, but only by `Server::run_udp_responder` on
  a socket the caller binds and drives, and only the broadcast `0x0001` form —
  `run_server` binds TCP alone).
- The server's accept loop serves one TCP connection at a time.
- Entity status and vehicle identification requests over TCP are logged and
  silently dropped (`ServerConnectionHandler` has no hook for them yet).
- The handler passed to `Server::new` is not validated.
- `Message::decode` accepts a header whose `payload_length` disagrees with what
  the payload actually occupies — it hands the payload exactly that many bytes
  and does not require them all to be consumed. `Message::encode` always
  derives the length field, so an emitted frame is self-consistent, but lenient
  decode remains. ISO 13400-2 has an entity answer an invalid payload length
  with NACK `0x04`, and `MessageError::PayloadLengthTooShort` exists, unused,
  for exactly this.
- `RoutingActivationRequest::encode` omits the optional vehicle-manufacturer
  field when it is `None`, writing 7 bytes instead of 11 — every golden vector
  agrees, but none exercises a peer that requires the long form
  (`src/messages/routing_activation_request.rs`).
- A failed `accept()` no longer panics the server task (fixed in 0.4.0): both
  the TCP accept loop and the UDP responder log the error, sleep briefly, and
  continue.

---

## 8. Invariants to preserve when changing this crate

1. **`tests/golden/` is frozen.** The 33 hex fixtures are the only external
   check that the wire format did not drift. Never regenerate them to make a
   test pass; a diff there means the change alters the wire format and needs a
   deliberate decision.
2. **The default build must stay allocator-free.**
   `cargo build --no-default-features` and
   `cargo run --example bare_metal_codec --no-default-features` are the guard.
3. **`try_frame` must never interpret a payload.** The moment it does, the
   sans-io seam (section 3) collapses and callers lose the ability to apply
   their own policy.
4. **There is exactly one encoder.** `Encode for OwnedMessage` delegates
   through `as_ref()`. Do not add a second serialization path.
5. **`automotive-wire-codec`'s semver is this crate's semver.** Bumping it is a
   public API change here.
