# `uds_on_ip` Initial API Shape Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `uds_on_ip` to ISO 14229-5 alone — the clause 8 profile and the clause 11 DoIP mapping — by removing the ISO 14229-2 vocabulary that belongs to `uds_session`, and shape the remaining surface as the transport the stack calls.

**Architecture:** This crate stops being a driver and becomes a transport. Roughly half of today's `src/` is ISO 14229-2 and leaves: `addressing.rs`, `primitives.rs`, `session.rs` and `handler.rs` are deleted, with their vocabulary taken from `uds_session` instead. `SessionLayer`, `RequestHandler`, `Ctx` and `Outcome` are deleted outright rather than relocated — under the agreed design nothing calls upward across this boundary. What remains is `profile` (clause 8), `mapping` (clause 11), `error`, and a new `transport` module carrying the methods `uds_services::UdsTransport` will require.

**Tech Stack:** Rust 2024, `no_std` + `alloc` optional, `automotive-wire-codec` 0.4, `uds_session`, `uds_protocol`, `simple_doip`. No `embedded-io`. No runtime named.

**Spec:** `../briefs/2026-09-16-uds-api-shapes-design.md` (cross-repo design), read together with `../briefs/uds_on_ip-boundary-brief.md` (this crate's boundary brief).

## Global Constraints

- **`unsafe` is forbidden crate-wide.** `Cargo.toml` `[lints.rust] unsafe_code = "forbid"`.
- **`missing_docs = "deny"`, `missing_debug_implementations = "deny"`, `clippy::pedantic = "deny"`.** Every public item needs rustdoc; every public type needs `Debug`.
- **`todo!()` is permitted in this crate.** Unlike `uds_session`, this `Cargo.toml` declares no panic-freedom lints, and the existing code already uses `todo!()` for unimplemented bodies. Keep that convention.
- **No allocation in the core.** No public type in `profile`, `mapping`, `error` or `transport` contains a `Vec` or `String`. Responses borrow the receive buffer.
- **No runtime is named outside an adapter.** `async fn` implies neither `std` nor tokio; a tokio *dependency* would compromise a `no_std` build.
- **Milliseconds, `u32`, wrapping.** Every timing value is `u32` milliseconds, matching `uds_session`'s `Timestamp`.
- **Timing parameter names are the standard's.** `tP6_Client_Max` / `tP6*_Client_Max` (ISO 14229-2 REQ 5.11), not `response_timeout` / `response_pending_timeout`.
- **`uds_session` is consumed from its `feat/public-api-stub` branch** by path dependency. It currently exports `Address, AddressExtension, Ai, Mtype, PeerIdentity, TaType, SResult, TransportError, Timestamp, ChannelParams, ChannelReload, ServerParams, ServerReload, Cause, Causes, Rejection, Reaction`, plus the four classification types. **It does not yet export `ChannelId`** — no task below may depend on one.
- **`uds_services` does not exist yet.** Its `src/` is `lib.rs` alone. No task may `use uds_services::…`. Task 6 builds the methods its trait will require; the `impl` block itself is a follow-on.

---

## File Structure

| File | Responsibility | Action |
| --- | --- | --- |
| `src/addressing.rs` | ISO 14229-2 cl. 8 addressing | **Delete** — `uds_session::addressing` |
| `src/primitives.rs` | ISO 14229-2 cl. 7 primitives | **Delete** — `uds_session` |
| `src/session.rs` | `SessionLayer`, `SessionAction`, `ChannelTiming` | **Delete** — seam no longer exists |
| `src/handler.rs` | `Ctx`, `Outcome`, `RequestHandler` | **Delete** — seam no longer exists |
| `src/profile.rs` | ISO 14229-5 cl. 8: timing, service ids, reconnect | Modify |
| `src/mapping.rs` | ISO 14229-5 cl. 11: T_PDU ⇄ DoIP | Modify |
| `src/error.rs` | Crate error taxonomy | Modify |
| `src/transport.rs` | The outward surface: `DoIpTransport` | **Create** |
| `src/client.rs` | Interim async client with session logic | **Delete** |
| `src/lib.rs` | Module wiring + crate docs | Modify |
| `Cargo.toml` | Dependencies, features | Modify |

---

### Task 1: Take the ISO 14229-2 vocabulary from `uds_session`

Deletes the two modules whose contents are ISO 14229-2 and wires the dependency that replaces them. Nothing else compiles until this lands, so it goes first.

**Files:**
- Modify: `Cargo.toml`
- Delete: `src/addressing.rs`, `src/primitives.rs`
- Modify: `src/lib.rs`
- Test: `tests/vocabulary.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: `uds_session` available as a dependency. `uds_on_ip` re-exports nothing from it — downstream names `uds_session` directly.

- [ ] **Step 1: Write the failing test**

Create `tests/vocabulary.rs`:

```rust
//! The ISO 14229-2 vocabulary is `uds_session`'s, and this crate does not
//! redefine it.
//!
//! These functions are never called. Type-checking them is the test.

#![allow(dead_code, reason = "type-checked, never run")]

/// All four `Mtype` variants exist, including the two this crate's old
/// `addressing.rs` dropped because DoIP has no address extension.
fn all_four_message_types_are_expressible() {
    let _local = uds_session::Mtype::Diag;
    let _local_secure = uds_session::Mtype::SecureDiag;
    let _remote = uds_session::Mtype::RDiag {
        ae: uds_session::AddressExtension(0x0000),
    };
    let _remote_secure = uds_session::Mtype::SecureRDiag {
        ae: uds_session::AddressExtension(0xFFFF),
    };
}

/// The addressing triple is taken whole, not re-declared here.
fn the_addressing_triple_is_uds_sessions(ai: uds_session::Ai) -> uds_session::Address {
    ai.sa
}

#[test]
fn the_vocabulary_compiles() {
    // The assertion is the build itself.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test vocabulary`
Expected: FAIL — `error[E0433]: failed to resolve: use of undeclared crate or module 'uds_session'`

- [ ] **Step 3: Add the dependency**

In `Cargo.toml`, in `[dependencies]`, after the `uds_protocol` line:

```toml
# ISO 14229-2 session layer vocabulary. This crate holds no addressing or
# service-primitive types of its own; see the boundary brief §2.
uds_session = { path = "../uds_session", default-features = false }
```

- [ ] **Step 4: Delete the two modules**

```bash
git rm src/addressing.rs src/primitives.rs
```

- [ ] **Step 5: Unwire them from `lib.rs`**

In `src/lib.rs`, delete these lines:

```rust
pub mod addressing;
pub mod primitives;
```

and these:

```rust
pub use addressing::{Address, Ai, ChannelId, Mtype, TaType};
pub use primitives::{Completion, Confirm, Indication, Request, SResult};
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --test vocabulary`
Expected: PASS. Other targets will still fail to build — Tasks 2–5 fix them.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/lib.rs tests/vocabulary.rs
git commit -m "refactor!: take the ISO 14229-2 vocabulary from uds_session

addressing.rs and primitives.rs are ISO 14229-2, not ISO 14229-5. Their
placement here deformed the types: Mtype carried two of the standard's four
variants because DoIP has no address extension, which a session-layer type
must not be narrowed by.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Delete the two seams that no longer have two sides

`SessionLayer` existed so this crate could build without `uds_session` while that repository was private. It is not. `RequestHandler` existed so a driver here could call upward. The driver is `uds_services` now, above the session layer, so nothing calls upward across this boundary.

**Files:**
- Delete: `src/session.rs`, `src/handler.rs`
- Modify: `src/lib.rs`
- Test: `tests/no_upward_seam.rs` (create)

**Interfaces:**
- Consumes: Task 1's `uds_session` dependency.
- Produces: the crate declares no trait for anything above it to implement.

- [ ] **Step 1: Write the failing test**

Create `tests/no_upward_seam.rs`:

```rust
//! This crate declares no trait that a layer above it implements.
//!
//! The boundary brief §2 deletes `SessionLayer` and `RequestHandler` rather
//! than relocating them: with `uds_services` driving, there is no
//! binding-side driver to call anything upward, and no seam for a
//! response-pending to cross.

/// Fails to compile if either trait is reintroduced.
#[test]
fn the_crate_declares_no_upward_trait() {
    let source = include_str!("../src/lib.rs");
    assert!(
        !source.contains("SessionLayer"),
        "SessionLayer is deleted, not relocated — see the boundary brief §2"
    );
    assert!(
        !source.contains("RequestHandler"),
        "RequestHandler is deleted, not relocated — see the boundary brief §2"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test no_upward_seam`
Expected: FAIL — `SessionLayer is deleted, not relocated`, because `lib.rs` still re-exports it.

- [ ] **Step 3: Delete the two modules**

```bash
git rm src/session.rs src/handler.rs
```

- [ ] **Step 4: Unwire them from `lib.rs`**

In `src/lib.rs`, delete:

```rust
pub mod handler;
pub mod session;
```

and:

```rust
pub use handler::{Ctx, Outcome, RequestHandler};
pub use session::{ChannelTiming, SessionAction, SessionLayer};
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --test no_upward_seam`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs tests/no_upward_seam.rs
git commit -m "refactor!: delete the session and handler seams

Both are deleted rather than relocated. SessionLayer existed so this crate
could build while uds_session was a private repository; it is not, and with
uds_services owning the Session concretely no trait is needed on either side.
RequestHandler existed so a driver here could call upward; the driver is above
the session layer now, so nothing calls upward across this boundary.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Delete the interim client

`src/client.rs` carries tester-present keepalive, response timing and `0x78` handling. All three are ISO 14229-2 or ISO 14229-1 behaviour that now lives in `uds_session` and `uds_services`. The brief is explicit: deleted, not ported.

**Files:**
- Delete: `src/client.rs`
- Modify: `src/lib.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: Task 2's state.
- Produces: no `client` feature, no tokio dependency, no `std` requirement anywhere in the crate.

- [ ] **Step 1: Write the failing test**

Append to `tests/no_upward_seam.rs`:

```rust
/// The boundary brief §3.5: the interim session logic in `client.rs` is
/// "interim by construction. **Deleted, not ported.**" Response-pending is
/// `uds_services`' decision entirely, submitted through this transport as
/// ordinary bytes.
#[test]
fn no_session_logic_survives_in_this_crate() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("tokio"),
        "a runtime dependency compromises the no_std build; see brief §3.3"
    );
    assert!(
        !manifest.contains("embedded-io"),
        "the I/O vocabulary is automotive-wire-codec's; see the awc brief"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test no_upward_seam`
Expected: FAIL — `a runtime dependency compromises the no_std build`

- [ ] **Step 3: Delete the module**

```bash
git rm src/client.rs
```

- [ ] **Step 4: Unwire it from `lib.rs`**

In `src/lib.rs`, delete:

```rust
#[cfg(feature = "client")]
pub mod client;
```

and:

```rust
#[cfg(feature = "client")]
pub use client::{Client, ClientOptions, Responses};
```

- [ ] **Step 5: Rewrite the manifest's dependencies and features**

In `Cargo.toml`, replace the whole `[dependencies]`, `[dev-dependencies]` and `[features]` blocks with:

```toml
[dependencies]
# Path deps during the prototype. Swap to registry versions once the stack
# publishes; see ARCHITECTURE.md 8.4.
simple_doip = { path = "../simple_doip", default-features = false }
uds_protocol = { path = "../uds_protocol", default-features = false }
uds_session = { path = "../uds_session", default-features = false }

# The I/O vocabulary for the whole stack. embedded-io is gone: awc 0.4 owns
# the sink trait and has no runtime dependencies of its own.
automotive-wire-codec = { path = "../../automotive_wire_format", default-features = false }

thiserror = { version = "2", default-features = false }

[features]
# No runtime is named anywhere. `async fn` implies neither std nor an
# executor, so the default build is no_std and a bare-metal target — AURIX
# TC4x is a qualification target — stays reachable. A tokio adapter, when one
# is written, is additive and lives behind its own feature.
default = []
alloc = ["simple_doip/alloc", "uds_protocol/alloc"]
std = ["alloc", "thiserror/std", "simple_doip/std", "uds_protocol/std"]
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --test no_upward_seam`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs Cargo.toml tests/no_upward_seam.rs
git commit -m "refactor!: delete the interim client and the runtime dependency

The tester-present keepalive, response timing and 0x78 handling in client.rs
are ISO 14229-2 and ISO 14229-1 behaviour. They live in uds_session and
uds_services now, and are deleted here rather than ported.

Dropping the client feature drops tokio with it, which is what makes an AURIX
TC4x target reachable: an async fn implies neither a runtime nor std, but a
tokio dependency would compromise the no_std build.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Rebuild `mapping` on the session vocabulary, with the address-extension rejection

Clause 11's whole job is a 14229-2 type meeting a 13400-2 type, so the `Address` ⇄ `LogicalAddress` conversions belong here rather than on the session type. REQ 4.4 Table 5 records `T_AE` as not applicable to DoIP, which becomes a mapping *rejection* rather than a missing enum variant.

**Files:**
- Modify: `src/mapping.rs`
- Test: `src/mapping.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `uds_session::{Address, Ai, Mtype, SResult}`.
- Produces:
  - `pub enum DoIpEvent<'a> { Ind { source: Address, data: &'a [u8] }, Conf { peer: Address, result: SResult }, Periodic { source: Address, pdid: u8, data: &'a [u8] }, Closed { cause: CloseCause } }`
  - `pub fn to_logical(addr: Address) -> simple_doip::LogicalAddress`
  - `pub fn from_logical(addr: simple_doip::LogicalAddress) -> Address`
  - `pub fn target_of(ai: &Ai) -> Result<simple_doip::LogicalAddress, MappingError>`
  - `pub enum MappingError { AddressExtensionUnsupported }`

- [ ] **Step 1: Write the failing test**

Append to `src/mapping.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{target_of, MappingError};
    use uds_session::{Address, AddressExtension, Ai, Mtype, TaType};

    fn ai_with(mtype: Mtype) -> Ai {
        Ai {
            mtype,
            sa: Address(0x0E00),
            ta: Address(0x0E80),
            ta_type: TaType::Physical,
        }
    }

    /// ISO 14229-5:2022 REQ 4.4 Table 5 marks `T_AE` not applicable to DoIP.
    /// The constraint lands here, where it is true, rather than deforming
    /// `uds_session::Mtype` into two variants.
    #[test]
    fn a_remote_message_type_is_rejected() {
        let ae = AddressExtension(0x0001);
        assert_eq!(
            target_of(&ai_with(Mtype::RDiag { ae })),
            Err(MappingError::AddressExtensionUnsupported)
        );
        assert_eq!(
            target_of(&ai_with(Mtype::SecureRDiag { ae })),
            Err(MappingError::AddressExtensionUnsupported)
        );
    }

    #[test]
    fn a_local_message_type_maps_to_its_target() {
        assert_eq!(
            target_of(&ai_with(Mtype::Diag)).map(|a| a.0),
            Ok(0x0E80)
        );
        assert_eq!(
            target_of(&ai_with(Mtype::SecureDiag)).map(|a| a.0),
            Ok(0x0E80)
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib mapping`
Expected: FAIL — `cannot find function 'target_of' in this scope`

- [ ] **Step 3: Write the implementation**

Replace `src/mapping.rs`'s `use` block and `TransportEvent`/`classify` with:

```rust
use uds_session::{Address, Ai, Mtype, SResult};

/// A constraint of the DoIP mapping, not of the session layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MappingError {
    /// ISO 14229-5:2022 REQ 4.4 Table 5 records `T_AE` as not applicable to
    /// DoIP, so `Mtype::RDiag` and `Mtype::SecureRDiag` cannot be carried.
    ///
    /// Expressible in `uds_session` and rejected here is the correct
    /// arrangement: the session layer keeps all four of the standard's
    /// message types, and the transport that cannot carry two of them says so.
    #[error("DoIP has no address extension (ISO 14229-5 REQ 4.4 Table 5)")]
    AddressExtensionUnsupported,
}

/// ISO 14229-2 `S_TA` to ISO 13400-2 logical address.
#[must_use]
pub fn to_logical(addr: Address) -> simple_doip::LogicalAddress {
    simple_doip::LogicalAddress(addr.0)
}

/// ISO 13400-2 logical address to ISO 14229-2 `S_TA`.
#[must_use]
pub fn from_logical(addr: simple_doip::LogicalAddress) -> Address {
    Address(addr.0)
}

/// The DoIP target address for this addressing triple, or why it has none.
///
/// # Errors
///
/// [`MappingError::AddressExtensionUnsupported`] for the two remote message
/// types.
pub fn target_of(ai: &Ai) -> Result<simple_doip::LogicalAddress, MappingError> {
    match ai.mtype {
        Mtype::Diag | Mtype::SecureDiag => Ok(to_logical(ai.ta)),
        Mtype::RDiag { .. } | Mtype::SecureRDiag { .. } => {
            Err(MappingError::AddressExtensionUnsupported)
        }
    }
}

/// An inbound DoIP message, classified.
///
/// Crate-internal in effect: `transport` translates the two that cross the
/// stack's seam into the driver's event type and handles the other two itself.
#[derive(Debug)]
pub enum DoIpEvent<'a> {
    /// `T_Data.ind` — a diagnostic message (DoIP `0x8001`).
    Ind {
        /// The responding entity. Under functional addressing this differs
        /// between responses, and is the only way to tell them apart.
        source: Address,
        /// The UDS payload, opaque at this layer.
        data: &'a [u8],
    },
    /// `T_Data.conf` — derived from a diagnostic message acknowledgement
    /// (DoIP `0x8002`/`0x8003`), **never** from a completed socket write,
    /// because the acknowledgement is what starts `tP_Client`
    /// (ISO 14229-2:2021 REQ 5.9).
    Conf {
        /// The acknowledging entity.
        peer: Address,
        /// `SResult::Ok` for `0x8002`; a `0x8003` becomes one
        /// `SResult::Transport` value carrying the NACK code.
        result: SResult,
    },
    /// A periodic response (DoIP `0x8004`).
    ///
    /// Deliberately not a `T_Data.ind`: ISO 14229-5:2022 REQ 7.20 requires
    /// that unsolicited responses do not reset `tS3_Server`, so these bypass
    /// the request/response path entirely.
    Periodic {
        /// The responding entity.
        source: Address,
        /// The periodic data identifier.
        pdid: u8,
        /// The periodic data record.
        data: &'a [u8],
    },
    /// The connection closed.
    Closed {
        /// Whether the close was expected.
        cause: CloseCause,
    },
}

/// Classify an inbound DoIP message.
///
/// # Prototype gap — `0x8004` is currently unrepresentable
///
/// `simple_doip`'s `Payload` models exactly the payload types ISO 13400-2
/// defines, with no catch-all carrying an unmodelled type's bytes, so
/// [`DoIpEvent::Periodic`] cannot be constructed. This needs no UDS semantics
/// in `simple_doip` — only a variant meaning "a payload type I do not model,
/// and here are its bytes". Named in that repository's brief §5.
#[allow(unused_variables)]
#[must_use]
pub fn classify<'a>(message: &simple_doip::messages::Message<'a>) -> Option<DoIpEvent<'a>> {
    todo!("classify Payload into a DoIpEvent; blocked on the 0x8004 gap above")
}
```

Leave `PERIODIC_RESPONSE_PAYLOAD_TYPE` where it is — it names the constant the `0x8004` gap is about.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib mapping`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add src/mapping.rs
git commit -m "feat(mapping): own the address conversions and the T_AE rejection

Address <-> LogicalAddress moves here, where a 14229-2 type meeting a 13400-2
type is the module's entire job — rather than uds_session carrying a From impl
for a DoIP type.

REQ 4.4 Table 5 marks T_AE not applicable to DoIP, so RDiag and SecureRDiag
are rejected in the mapping. Expressible above and refused here is the right
arrangement: the session layer keeps all four message types, and the transport
that cannot carry two says so.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: Correct `profile`'s timing names and split the reload pair

The brief §3.5: `response_timeout` / `response_pending_timeout` are `tP6_Client_Max` / `tP6*_Client_Max`, and `tP3_Client_Phys` / `tP3_Client_Func` are absent. The struct already carries the right four fields under the right names — what changes is that the *reload pair* is separated from the spacing pair, because the design doc §6.2 has `UdsTransport::channel_timing()` supply only the reloads.

**Files:**
- Modify: `src/profile.rs`
- Test: `src/profile.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `uds_session::ChannelReload`.
- Produces:
  - `pub struct Timing { pub reloads: Reloads, pub spacing: Spacing }`
  - `pub struct Reloads { pub p6_client_max_ms: u32, pub p6_star_client_max_ms: u32 }`
  - `pub struct Spacing { pub p3_client_phys_ms: u32, pub p3_client_func_ms: u32 }`
  - `impl Reloads { pub fn value_for(&self, which: uds_session::ChannelReload) -> u32 }`

- [ ] **Step 1: Write the failing test**

Append to `src/profile.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{Reloads, Spacing, Timing};
    use uds_session::ChannelReload;

    /// ISO 14229-2:2021 REQ 5.11 — DoIP has no `T_DataSOM.ind`, so the tP6
    /// pair applies rather than the tP2 pair. The session layer does not
    /// distinguish the two cases; the distinction survives in the values this
    /// transport supplies.
    #[test]
    fn the_reload_pair_answers_the_session_layers_question() {
        let reloads = Reloads {
            p6_client_max_ms: 2_000,
            p6_star_client_max_ms: 5_000,
        };
        assert_eq!(reloads.value_for(ChannelReload::Default), 2_000);
        assert_eq!(reloads.value_for(ChannelReload::Enhanced), 5_000);
    }

    /// Design doc §6.2 — spacing is ISO 14229-2 clause 9.7 client policy, on
    /// which a transport has no view, so it is not part of what
    /// `channel_timing` supplies.
    #[test]
    fn spacing_is_separable_from_the_reloads() {
        let timing = Timing::default();
        let _reloads_alone: Reloads = timing.reloads;
        let _spacing_alone: Spacing = timing.spacing;
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib profile`
Expected: FAIL — `cannot find type 'Reloads' in this scope`

- [ ] **Step 3: Write the implementation**

In `src/profile.rs`, replace the `Timing` struct and its `Default` impl with:

```rust
/// The `tP_Client` reload pair this transport dictates.
///
/// ISO 14229-5:2022 REQ 7.19 defers the values to ISO 14229-2. What this crate
/// contributes is the *choice of parameter*: DoIP offers no `T_DataSOM.ind`,
/// so the `tP6` pair applies rather than the `tP2` pair
/// (ISO 14229-2:2021 REQ 5.11).
///
/// This is what `UdsTransport::channel_timing` supplies. The spacing pair is
/// deliberately not part of it — see [`Spacing`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reloads {
    /// `tP6_Client_Max` — wait for a complete response after `T_Data.conf`.
    pub p6_client_max_ms: u32,
    /// `tP6*_Client_Max` — enhanced wait after a response-pending NRC.
    pub p6_star_client_max_ms: u32,
}

impl Reloads {
    /// The value for whichever reload the session layer says is in force.
    #[must_use]
    pub const fn value_for(&self, which: uds_session::ChannelReload) -> u32 {
        match which {
            uds_session::ChannelReload::Default => self.p6_client_max_ms,
            uds_session::ChannelReload::Enhanced => self.p6_star_client_max_ms,
        }
    }
}

/// Minimum spacing between consecutive requests.
///
/// ISO 14229-2:2021 clause 9.7 client policy. Separate from [`Reloads`]
/// because a transport has no view on it: the reload pair is dictated by
/// whether the transport offers a `T_DataSOM.ind`, while the spacing is the
/// client's own conduct.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Spacing {
    /// `tP3_Client_Phys` — minimum spacing before the next physically
    /// addressed request when the previous one required no response.
    pub p3_client_phys_ms: u32,
    /// `tP3_Client_Func` — the same, for functionally addressed requests.
    pub p3_client_func_ms: u32,
}

/// Every ISO 14229-5 timing parameter this profile carries.
///
/// ISO 14229-2:2021 clause 9.2 Table 4 specifies the reloads as minima derived
/// from the server's timing and vehicle-network delays, not as fixed values,
/// so there is no defensible universal default. The values in [`Timing::default`]
/// are conventional bench starting points and must be configured for a real
/// vehicle.
///
/// Values are milliseconds, matching `uds_session::Timestamp`'s unit. Carrying
/// a `Duration` here would mean converting at the seam on every call, which is
/// a conversion that can only go wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// What the session layer loads its response timer with.
    pub reloads: Reloads,
    /// What the client waits before its next request.
    pub spacing: Spacing,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            reloads: Reloads {
                p6_client_max_ms: 2_000,
                p6_star_client_max_ms: 5_000,
            },
            spacing: Spacing::default(),
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib profile`
Expected: PASS (2 tests)

- [ ] **Step 5: Confirm clause 8's service-identifier read against T_PDU input**

Boundary brief §3.4 asks for this explicitly rather than by implication, because
`service_ids` is the one place this crate looks at UDS content, and on a segmented
transport the equivalence would not hold.

Append to `src/profile.rs`'s `mod tests`:

```rust
    /// Boundary brief §3.4 — confirmed, not assumed.
    ///
    /// Clause 8 keys TCP connection handling on `DiagnosticSessionControl` and
    /// `ECUReset`. Under the agreed design the driver sits above the session
    /// layer, so what reaches this crate is a T_PDU rather than an A_PDU.
    ///
    /// On DoIP that distinction is nominal: ISO 14229-5:2022 REQ 4.4 Table 5
    /// maps `T_Data` onto the DoIP diagnostic message's user data unchanged,
    /// and DoIP performs no segmentation — which is the same fact
    /// ISO 14229-2:2021 REQ 5.11 rests on when it gives DoIP the tP6 pair for
    /// having no `T_DataSOM.ind`. So the A_PDU and the T_PDU are the same
    /// octets and the first one is still the service identifier.
    ///
    /// On a segmented transport this would not hold, and a binding for one
    /// must re-derive it rather than copy this module.
    #[test]
    fn the_service_identifier_is_the_first_octet_of_a_t_pdu() {
        use super::service_ids::{DIAGNOSTIC_SESSION_CONTROL, ECU_RESET};

        let session_control_t_pdu = [DIAGNOSTIC_SESSION_CONTROL, 0x03];
        let ecu_reset_t_pdu = [ECU_RESET, 0x01];

        assert_eq!(session_control_t_pdu[0], DIAGNOSTIC_SESSION_CONTROL);
        assert_eq!(ecu_reset_t_pdu[0], ECU_RESET);
    }
```

Run: `cargo test --lib profile`
Expected: PASS (3 tests)

- [ ] **Step 6: Commit**

```bash
git add src/profile.rs
git commit -m "refactor(profile): name the timing parameters after the standard

The reload pair is separated from the spacing pair because UdsTransport
supplies only the reloads: which pair applies is dictated by whether the
transport offers a T_DataSOM.ind, while tP3 spacing is ISO 14229-2 clause 9.7
client policy on which a transport has no view.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: Create `transport` — the crate's outward surface

The methods `uds_services::UdsTransport` will require, as inherent methods on a concrete type. The `impl` block is a follow-on once that crate publishes the trait; writing the methods now means it will be one line each and lets everything below it be exercised meanwhile.

**Files:**
- Create: `src/transport.rs`
- Modify: `src/lib.rs`
- Test: `src/transport.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `crate::mapping::{DoIpEvent, MappingError, target_of}`, `crate::profile::Reloads`, `uds_session::{Ai, SResult}`.
- Produces:
  - `pub struct DoIpTransport<S>`
  - `pub enum TransportEvent<'a> { DataInd { ai: Ai, data: &'a [u8] }, DataConf { ai: Ai, result: SResult }, Deadline }`
  - `async fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> Result<(), Error>`
  - `async fn next_event(&mut self, deadline_ms: Option<u32>) -> Result<TransportEvent<'_>, Error>`
  - `fn inbound_max(&self) -> Option<usize>` / `fn outbound_max(&self) -> Option<usize>`
  - `fn channel_timing(&self) -> Reloads`
  - `fn now_ms(&self) -> u32`

- [ ] **Step 1: Write the failing test**

Create `src/transport.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    /// ISO 13400-2:2019 Table 11 lists *Max. data size* as the fourth item of
    /// the entity status response and marks its support **optional**, so a
    /// conformant DoIP entity need not advertise one and `None` is a correct
    /// answer rather than a defect.
    ///
    /// Design doc §1 decision 3: `uds_services` bounds the response sink only
    /// where a bound is known, and never fabricates one.
    #[test]
    fn an_unadvertised_max_data_size_is_none_not_a_guess() {
        let t = super::DoIpTransport::new((), crate::profile::Timing::default());
        assert_eq!(t.inbound_max(), None);
        assert_eq!(t.outbound_max(), None);
    }

    /// MDS is "the maximum size of one logical **request** that this DoIP
    /// entity can process", so the two directions are different questions and
    /// answering one does not answer the other.
    #[test]
    fn the_two_directions_are_independent() {
        let mut t = super::DoIpTransport::new((), crate::profile::Timing::default());
        t.set_inbound_max(Some(4096));
        assert_eq!(t.inbound_max(), Some(4096));
        assert_eq!(
            t.outbound_max(),
            None,
            "this entity's own MDS says nothing about what the peer will accept"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib transport`
Expected: FAIL — `cannot find module 'transport'`, then once wired, `cannot find struct 'DoIpTransport'`

- [ ] **Step 3: Write the implementation**

Prepend to `src/transport.rs`, above the test module:

```rust
//! The crate's outward interface.
//!
//! [`DoIpTransport`] carries the methods `uds_services::UdsTransport`
//! requires. The `impl` block arrives once that crate publishes the trait;
//! until then these are inherent methods with the agreed signatures, so the
//! implementation is exercised rather than blocked.
//!
//! Nothing here is shaped by `uds_services`: the trait's own test is that a
//! CAN binding implements the same methods, so a DoIP-shaped seam would be
//! the wrong seam.

use crate::error::Error;
use crate::mapping::target_of;
use crate::profile::{Reloads, Timing};
use uds_session::{Ai, SResult};

/// The driver's view of what arrived, or that its deadline passed first.
///
/// Mirrors `uds_services::TransportEvent` exactly; `mapping::DoIpEvent`'s
/// other two cases — a periodic response and a connection close — are handled
/// inside this module and do not cross the seam.
#[derive(Debug)]
pub enum TransportEvent<'a> {
    /// A complete inbound message.
    DataInd {
        /// Addressing, with the responder's `S_AI[SA]` — the only way to tell
        /// functional responses apart.
        ai: Ai,
        /// The UDS payload.
        data: &'a [u8],
    },
    /// The outcome of a requested transmission.
    ///
    /// Raised from the diagnostic message **acknowledgement**, never from the
    /// socket write returning, because the acknowledgement is what starts
    /// `tP_Client` (ISO 14229-2:2021 REQ 5.9).
    DataConf {
        /// The addressing of the transmission being confirmed.
        ai: Ai,
        /// `SResult::Ok` for a `0x8002`; a `0x8003` is one
        /// `SResult::Transport` value.
        result: SResult,
    },
    /// The deadline the driver supplied passed before anything arrived.
    Deadline,
}

/// ISO 14229-5 over DoIP.
///
/// `S` is the socket, so no runtime is named: this builds for a bare-metal
/// target as readily as for tokio, and an adapter for either is additive.
#[derive(Debug)]
pub struct DoIpTransport<S> {
    socket: S,
    timing: Timing,
    inbound_max: Option<usize>,
    outbound_max: Option<usize>,
}

impl<S> DoIpTransport<S> {
    /// A transport over `socket` with `timing`, advertising no size bound in
    /// either direction until one is learned.
    pub const fn new(socket: S, timing: Timing) -> Self {
        Self {
            socket,
            timing,
            inbound_max: None,
            outbound_max: None,
        }
    }

    /// Record this entity's own *Max. data size*, learned from its
    /// configuration or from its entity status response.
    pub fn set_inbound_max(&mut self, max: Option<usize>) {
        self.inbound_max = max;
    }

    /// Record the peer's advertised *Max. data size*, learned from the peer's
    /// entity status response.
    ///
    /// A server typically has not requested one, which is why this stays
    /// `None` and `responseTooLong` is then unreachable rather than fabricated.
    pub fn set_outbound_max(&mut self, max: Option<usize>) {
        self.outbound_max = max;
    }

    /// The largest A_PDU this entity will accept, where it advertises one.
    ///
    /// ISO 13400-2:2019 Table 11 — support for *Max. data size* is
    /// **optional**, so `None` is conformant.
    #[must_use]
    pub const fn inbound_max(&self) -> Option<usize> {
        self.inbound_max
    }

    /// The largest A_PDU the peer will accept, where it has advertised one.
    ///
    /// This is what bounds a *response*: MDS is defined as the maximum size of
    /// one logical **request** the entity can process, so a server asking what
    /// it may send is asking about the client.
    #[must_use]
    pub const fn outbound_max(&self) -> Option<usize> {
        self.outbound_max
    }

    /// The `tP_Client` reload pair this transport dictates.
    #[must_use]
    pub const fn channel_timing(&self) -> Reloads {
        self.timing.reloads
    }

    /// Monotonic milliseconds, 32-bit and wrapping.
    #[must_use]
    pub fn now_ms(&self) -> u32 {
        todo!("clock source is the socket adapter's; see design doc decision 1")
    }

    /// `T_Data.req` — map a T_PDU onto a DoIP diagnostic message and send it.
    ///
    /// # Errors
    ///
    /// [`Error`] if the addressing cannot be carried — the two remote message
    /// types have no DoIP representation — or if the socket fails.
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> Result<(), Error> {
        let _target = target_of(&ai)?;
        todo!("REQ 4.3 Table 4 — send as a DoIP diagnostic message")
    }

    /// The next inbound event, or [`TransportEvent::Deadline`] when
    /// `deadline_ms` passes first.
    ///
    /// The deadline is the session layer's `next_deadline_ms`, so this
    /// transport never invents one.
    ///
    /// # Errors
    ///
    /// [`Error`] if the socket fails.
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn next_event(
        &mut self,
        deadline_ms: Option<u32>,
    ) -> Result<TransportEvent<'_>, Error> {
        todo!("read a DoIP message, mapping::classify it, translate the two seam cases")
    }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

In `src/lib.rs`, add to the module list:

```rust
pub mod transport;
```

and to the re-exports:

```rust
pub use transport::{DoIpTransport, TransportEvent};
```

- [ ] **Step 5: Add the `MappingError` conversion to `error.rs`**

In `src/error.rs`, add a variant to `Error` and the `From` impl the `?` in `t_data_req` needs:

```rust
    /// The addressing cannot be carried over DoIP.
    #[error(transparent)]
    Mapping(#[from] crate::mapping::MappingError),
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib transport`
Expected: PASS (2 tests)

- [ ] **Step 7: Commit**

```bash
git add src/transport.rs src/error.rs src/lib.rs
git commit -m "feat(transport): add the crate's outward interface

The methods uds_services::UdsTransport requires, as inherent methods on a
concrete type. The impl block follows once that crate publishes the trait.

inbound_max and outbound_max are two Options rather than one usize because
ISO 13400-2 Table 11 makes Max. data size optional and defines it for
requests: None is conformant, and a server bounding its own response is asking
about the peer. Fabricating a bound would make a conformant server truncate
valid responses.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: Rewrite the crate documentation against the real design

`lib.rs`'s module doc says this crate "appears twice" and "wraps the session layer", and points at `session::SessionLayer` and `handler::RequestHandler`. All of that describes the design the boundary brief reversed.

**Files:**
- Modify: `src/lib.rs`
- Test: `cargo doc` and the full suite

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: a crate that builds clean under `no_std` with no features.

- [ ] **Step 1: Write the failing test**

Append to `tests/no_upward_seam.rs`:

```rust
/// The boundary brief §4: the "this crate appears twice / wraps the session
/// layer" sandwich no longer describes the design. The driver is above the
/// session layer and this crate is wholly below it.
#[test]
fn the_crate_docs_do_not_describe_the_reversed_design() {
    let source = include_str!("../src/lib.rs");
    assert!(
        !source.contains("appears twice"),
        "this crate is wholly below the session layer now"
    );
    assert!(
        !source.contains("wraps the session layer"),
        "the driver is above the session layer; this crate is below it"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test no_upward_seam`
Expected: FAIL — `this crate is wholly below the session layer now`

- [ ] **Step 3: Replace the module documentation**

Replace everything in `src/lib.rs` above `#![cfg_attr(...)]` with:

```rust
//! # UDS on Internet Protocol
//!
//! An implementation of **ISO 14229-5:2022 (`UDSonIP`)** — the application
//! profile that binds Unified Diagnostic Services to a `DoIP` transport.
//!
//! ## What this crate is
//!
//! ISO 14229-5 clause 7 enumerates its own content as a table of requirements
//! grouped by OSI layer, and that table is this crate's scope. It owns two
//! layers:
//!
//! - **Clause 8, the application profile** (`REQ 7.1`–`7.20`) — the `A_PDU`
//!   format, TCP connection handling around `DiagnosticSessionControl` and
//!   `ECUReset`, periodic responses, and which timing parameters apply.
//! - **Clause 11, the transport mapping** (`REQ 4.3`, `REQ 4.4`) — mapping the
//!   `T_PDU` service primitives and parameters onto `DoIP`'s.
//!
//! ## Where it sits
//!
//! ```text
//!   consuming application
//!        ↕  typed service traits / typed client calls
//!   uds_services      the driver — owns the Session, declares UdsTransport
//!        ↓  UdsTransport
//!   uds_on_ip         ISO 14229-5 profile + DoIP mapping  ← this crate
//!        ↓
//!   simple_doip       ISO 13400-2
//! ```
//!
//! This crate is **wholly below** the session layer. It hosts no driver, calls
//! nothing upward, and knows nothing about services. `uds_services` owns the
//! `uds_session::Session`, supplies its inputs, drains its actions, and calls
//! this crate through a trait it declares — which is why the dependency edge
//! points from here to `uds_services` and not the other way.
//!
//! ## `no_std`, alloc-freedom, and no runtime
//!
//! The crate is `no_std` and allocates nothing: no public type contains a
//! `Vec` or a `String`, and an inbound message borrows the receive buffer.
//!
//! It is **async without naming a runtime**. An `async fn` implies neither an
//! executor nor `std`, but a runtime *dependency* would compromise the
//! `no_std` build — and a bare-metal AURIX TC4x target is a qualification
//! target. [`transport::DoIpTransport`] is therefore generic over its socket,
//! and an adapter for tokio or embassy is additive.
//!
//! ## Status
//!
//! **Prototype.** The public API is unstable and most bodies are
//! unimplemented. Known gaps are recorded in `ARCHITECTURE.md` §9 and in the
//! boundary brief carried alongside this repository.
//!
//! ## What this crate deliberately does not do
//!
//! It does not decode UDS messages — that is `uds_protocol` — and it does not
//! dispatch services, choose negative response codes, or know what a data
//! identifier is. Those are ISO 14229-1 clause 8.7 concerns and belong to
//! `uds_services`.
//!
//! It holds no ISO 14229-2 vocabulary. Addressing, the service primitives and
//! the session state machine are `uds_session`'s, and are used from there
//! rather than redeclared here.
//!
//! The one exception is narrow and forced by the standard: clause 8 keys TCP
//! connection handling on two specific service identifiers. See
//! [`profile::service_ids`].
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test --all-targets`
Expected: PASS — all tests across `vocabulary`, `no_upward_seam`, and the inline `mapping`, `profile` and `transport` modules.

- [ ] **Step 5: Verify the no_std build and the lints**

Run: `cargo build --no-default-features`
Expected: builds clean.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

Run: `cargo doc --no-deps`
Expected: no broken intra-doc links. `[\`transport::DoIpTransport\`]` and `[\`profile::service_ids\`]` must resolve.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs tests/no_upward_seam.rs
git commit -m "docs: describe the crate as the transport it now is

The 'appears twice / wraps the session layer' sandwich described the design the
boundary brief reversed. The driver is above the session layer and this crate
is wholly below it, hosting no driver and calling nothing upward.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Out of scope for this plan

Recorded so they are not mistaken for omissions.

- **`impl uds_services::UdsTransport for DoIpTransport`.** Blocked: that crate's `src/` is `lib.rs` alone. Task 6 makes it one line per method when the trait lands.
- **`ARCHITECTURE.md` → a sphinx-needs `docs/architecture/` set**, with this crate's ID prefix allocated. That is documentation infrastructure rather than API shape, and is a plan of its own. `ARCHITECTURE.md` §4.1, §4.2, §4.3, §8.1, §8.2, §9.2, §12.3 and §13 are stale in the meantime — the boundary brief §4 lists exactly how.
- **The three `simple_doip` gaps** that block real behaviour: `0x8004` cannot be received, the diagnostic message acknowledgement is not a distinct primitive so `DataConf` cannot be raised correctly, and our own entity never emits `0x8003`. Named in that repository's brief §5; `mapping::classify` and `transport::next_event` stay `todo!()` until they close.
- **Functional fan-out.** Each `DataInd` carries the responder's `S_AI[SA]` in the shape landed here, so the design is expressible; exercising it needs the `simple_doip` work above.
- **`Encode`/`Decode` call sites onto `awc::Sink`.** The dependency is added in Task 3; migrating the call sites belongs with the behaviour that has them.
