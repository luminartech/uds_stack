// Derogations from the workspace lint standard in the root Cargo.toml. Each is a
// gap to close, not a decision that the lint is wrong here.
//
// The defensive three, `indexing_slicing`, `arithmetic_side_effects` and
// `as_conversions`, are allowed on each module below that predates the standard, and on
// no other: 13 sites in production code as of this writing.
// Panic freedom holds in this crate's production code and is enforced there; its
// test modules predate the standard and use `unwrap` as test code ordinarily does
// (125 sites). Scoped to `test`, so the production build stays strict.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! # Simple `DoIP`
//!
//! An implementation of Diagnostics over IP (`DoIP`), the vehicle-diagnostics transport
//! specified in [ISO 13400-2:2019](https://www.iso.org/standard/74785.html). A table,
//! figure, clause or requirement cited in this crate with no document named is that
//! one's, and any other document is named where it is cited.
//!
//! ## Design
//!
//! The protocol core is `no_std` and zero-copy: [`messages::Message`] borrows directly
//! from the receive buffer and never allocates. Wire primitives come from
//! [`automotive_wire_codec`], re-exported as [`wire`] so consumers do not need their own
//! dependency on it.
//!
//! Capability is layered by Cargo feature, each building on the previous:
//!
//! | Feature | Adds |
//! |---|---|
//! | *(none)* | `no_std` borrowed messages, [`try_frame`] framing, encode/decode |
//! | `alloc` | Owned mirrors (`messages::OwnedMessage`) that outlive the receive buffer |
//! | `std` | `std`-backed I/O and error traits |
//! | `codec` | `message_codec::MessageCodec`, a `tokio-util` `Encoder`/`Decoder` |
//! | `connection` | The `no_std` connection service over `edge-nal`: `tester::Tester` and `entity::Entity` |
//!
//! `default = []`, so an embedded target gets the `no_std` core with no allocator and no
//! runtime. `connection` stands apart from the chain above: it is `no_std`, allocates
//! nothing, and needs none of the other features.
//!
//! ## The `connection` feature
//!
//! Performs I/O through [`edge-nal`](https://docs.rs/edge-nal/0.7) and keeps time with
//! [`embassy-time`](https://docs.rs/embassy-time/0.5). This crate declares no transport or
//! clock trait of its own, so the integrator supplies both.
//!
//! 1. **An `edge-nal` 0.7 backend**: `TcpConnect` for a tester, `TcpBind` for an entity,
//!    whose bound acceptor the entity borrows. For discovery, a bound `UdpSplit`
//!    socket able to send to the limited broadcast address: an entity's on
//!    [`UDP_DISCOVERY_PORT`], a tester's on a port in 49152 to 65535, which the
//!    integrator picks (REQ 4.DoIP-135).
//!    `edge-nal-std` serves a host; on bare metal the backend is the integrator's.
//! 2. **An `embassy-time` driver *and* a timer queue.** These are two settings, and
//!    missing either is a *link* error, not a compile error. On a host:
//!    `embassy-time = { version = "0.5", features = ["std", "generic-queue-8"] }`. On
//!    bare metal the driver usually comes with the HAL. Tests can use its `mock-driver`.
//! 3. **Reads, writes, readiness and accepts that do nothing when cancelled.** The
//!    event methods of this feature are cancel-safe — a caller may drop them at any
//!    await and call again — only if the backend's `embedded_io_async::Read::read`,
//!    `embedded_io_async::Write::write`, `edge_nal::Readable::readable` and, for an
//!    entity, `edge_nal::TcpAccept::accept` move no data and change no connection state
//!    when dropped before completing, nor do a UDP socket's
//!    `edge_nal::UdpReceive::receive` and `edge_nal::UdpSend::send` move a datagram; and
//!    `edge_nal::TcpShutdown::close` and `abort`, which a dropped close calls again, may
//!    be called again. For an entity this holds of the halves `edge_nal::TcpSplit::split`
//!    and `edge_nal::UdpSplit::split` give too, which it reads and writes at the same
//!    time: each half must keep its own wakeup, so that one waiting does not displace
//!    the other's, and a TCP read half's `readable` must complete at end of stream, as
//!    the socket's does. `embedded-io-async` encourages that but does not require it,
//!    so it is the integrator's obligation:
//!    - `edge-nal-std` 0.7.0 meets it for all of these, and for the halves.
//!    - `edge-nal-embassy` 0.9.0 meets it for `read`, `write`, `readable`, `close` and
//!      `abort`, but its read half's `readable` misses an end of stream with nothing
//!      buffered, and **its `accept` is not cancel-safe**: it creates its socket inside
//!      the future, so a cancelled `accept` drops a connection that may already be
//!      established. An entity on embassy-net accepts through an adapter over
//!      embassy-net's own `TcpSocket` instead; the repository's
//!      `examples/embassy-net-entity` crate is one. Its UDP socket was not checked, and
//!      that crate adapts embassy-net's own `UdpSocket`, which meets it.
//! 4. **Reads and writes that wait on nothing else.** A `read` after `readable` has
//!    reported data completes with it, as does a UDP `receive` after `readable`, and a
//!    `write` sends what it accepts without waiting for
//!    `embedded_io_async::Write::flush`, which this feature never calls. A backend that
//!    holds data back, such as a record layer buffering a whole record, holds a request
//!    until it is lost, and a passed deadline until the data comes.
//!    `edge-nal-std` 0.7.0 and `edge-nal-embassy` 0.9.0 meet it.
//! 5. **For an entity, `MCTS + 1` sockets** from the backend: ISO 13400-2:2019 Table 11
//!    counts the maximum concurrent `TCP_DATA` sockets excluding the reserve socket, on
//!    which a further connection is accepted or refused.
//!
//! ## Where to start
//!
//! - **A `no_std` tester:** `tester::discovery` finds the entities on a network and
//!   asks one its status; `tester::Tester` connects to an entity, activates routing
//!   and is then a [`service::TesterConnection`], a [`service::DiagnosticConnection`]
//!   that can reconnect and close (requires the `connection` feature).
//! - **Writing against the connection service:** [`service`] holds ISO 13400-2's own
//!   service vocabulary, the [`service::DiagnosticConnection`] trait for one
//!   connection and the [`service::DiagnosticEntity`] trait for a whole entity, with
//!   no I/O.
//! - **A `no_std` entity:** `entity::Entity` is an entity's `TCP_DATA` sockets behind
//!   [`service::DiagnosticEntity`], over a bound `edge-nal` acceptor, and with
//!   `entity::Entity::with_discovery` its `UDP_DISCOVERY` socket too: announcement,
//!   identification, entity status and power mode (requires the `connection` feature).
//! - **Bare metal / sans-io:** [`try_frame`] delimits a frame from a byte buffer without
//!   owning any I/O resource; [`messages::Payload::decode`] then interprets the body.
//!   See `examples/bare_metal_codec.rs`.

#![no_std]
#![warn(missing_docs, missing_debug_implementations)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "connection")]
pub mod entity;
pub mod identifiers;
pub mod logical_address;
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions
)]
pub mod messages;
pub mod service;
pub mod wire;
pub use identifiers::{EntityId, GroupId, NotSet, Vin, VinError};
pub use logical_address::{LogicalAddress, TaType};
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions
)]
mod framer;
pub use framer::{RawFrame, try_frame};

#[cfg(feature = "codec")]
pub mod message_codec;
#[cfg(feature = "connection")]
mod stream;
#[cfg(feature = "connection")]
pub mod tester;

use core::time::Duration;

/// Default TCP port for `DoIP`
/// This is the port used for unencrypted connections
/// Used for:
///  * Vehicle information services
///  * Control commands
///
pub const TCP_PORT: u16 = 13400;

/// Default UDP port for `DoIP`
/// This is the port used for discovery
pub const UDP_DISCOVERY_PORT: u16 = 13400;

/// TCP port for `DoIP` over TLS, per ISO 13400-2. Not currently used by this
/// crate: connections are established in the clear via [`TCP_PORT`]; there is no
/// TLS support yet.
pub const TCP_TLS_PORT: u16 = 3496;

// DoIP timing and communication parameters

/// Initial inactivity timeout in seconds for TCP connections directly after a `TCP_DATA`
/// socket is established. Timeout is 2 seconds.
///
/// Must complete routing activation within this time otherwise the socket is closed by the
/// `DoIP` entity
pub const TCP_TIMEOUT_INITIAL_INACTIVITY: Duration = Duration::from_secs(2);

/// General inactivity timeout for TCP connections. Timeout is 300 seconds (5 minutes).
///
/// If no data is sent or received for this duration, the connection is closed by the `DoIP`
/// entity
pub const TCP_TIMEOUT_GENERAL_INACTIVITY: Duration = Duration::from_secs(300);

/// `T_TCP_Alive_Check`: how long an entity waits for an alive check response after
/// writing an alive check request on a `TCP_DATA` socket (ISO 13400-2:2019 Table 12).
pub const TCP_TIMEOUT_ALIVE_CHECK: Duration = Duration::from_millis(500);

/// Time between receipt of the last byte of a `DoIP` Diagnostic Message and transmission of
/// the ACK or NACK.
///
/// This is a performance requirement on the **entity emitting the ACK**, not a
/// deadline for a tester waiting on one. Do not use it to time out a send:
/// it allows nothing for network transit or for an entity that ACKs after
/// running its handler, and an entity that is merely slow is not an entity
/// that failed. [`TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE`] is the parameter that
/// governs when a message may be considered lost.
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL: Duration = Duration::from_millis(50);

/// After the timeout has elapsed, the request or response is considered to be lost and the
/// request may be repeated
///
/// Ref: `A_DoIP_Diagnostic_Message`
pub const TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE: Duration = Duration::from_secs(2);

/// `A_DoIP_Ctrl`: how long a tester waits for the answers to a UDP request (Table 12).
pub const A_DOIP_CTRL: Duration = Duration::from_secs(2);

/// `A_DoIP_Announce_Wait`'s upper bound: an entity waits a random time up to it before
/// answering a vehicle identification request, and before its first announcement
/// (Table 12).
pub const A_DOIP_ANNOUNCE_WAIT_MAX: Duration = Duration::from_millis(500);

/// `A_DoIP_Announce_Interval`: the time between an entity's announcements (Table 12).
pub const A_DOIP_ANNOUNCE_INTERVAL: Duration = Duration::from_millis(500);

/// `A_DoIP_Announce_Num`: how many announcements an entity sends (Table 12).
pub const A_DOIP_ANNOUNCE_NUM: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    /// ISO 13400-2:2019 Table 12: `T_TCP_Alive_Check` times out after 500 ms.
    #[test]
    fn alive_check_timeout_is_table_12s() {
        assert_eq!(TCP_TIMEOUT_ALIVE_CHECK, Duration::from_millis(500));
    }
}
