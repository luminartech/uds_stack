//! # UDS on Internet Protocol
//!
//! An implementation of **ISO 14229-5:2022 (`UDSonIP`)** — the application
//! profile that binds Unified Diagnostic Services to a `DoIP` transport.
//!
//! ## What this crate is
//!
//! ISO 14229-5 clause 7 enumerates its own content as a table of requirements
//! grouped by OSI layer, and that table is this crate's scope. It owns two
//! layers of the stack:
//!
//! - **Clause 8, the application profile** (`REQ 7.1`–`7.20`) — the `A_PDU`
//!   format, TCP connection handling around `DiagnosticSessionControl` and
//!   `ECUReset`, periodic responses, and which timing parameters apply.
//! - **Clause 11, the transport mapping** (`REQ 4.3`, `REQ 4.4`) — mapping the
//!   `T_PDU` service primitives and parameters onto `DoIP`'s.
//!
//! Everything between those two layers is the session layer, which this crate
//! *drives* but does not implement (see `session`).
//!
//! ## The shape this produces
//!
//! ```text
//!   application
//!        │ A_Data
//!   ┌────▼─────────────────────┐
//!   │ uds_on_ip · clause 8     │  application profile
//!   └────┬─────────────────────┘
//!        │ S_Data
//!   ┌────▼─────────────────────┐
//!   │ uds_session              │  ISO 14229-2
//!   └────┬─────────────────────┘
//!        │ T_Data
//!   ┌────▼─────────────────────┐
//!   │ uds_on_ip · clause 11    │  transport mapping
//!   └────┬─────────────────────┘
//!        │ `DoIP_Data`
//!   ┌────▼─────────────────────┐
//!   │ simple_doip              │  ISO 13400-2
//!   └──────────────────────────┘
//! ```
//!
//! This crate appears twice. It **wraps** the session layer rather than
//! stacking on top of it, which is why `session::SessionLayer` is driven from
//! both directions. A full discussion is in `ARCHITECTURE.md`.
//!
//! ## `no_std` and alloc-freedom
//!
//! The core — `addressing`, `session`, [`mapping`], [`profile`],
//! `handler` — is `no_std` and allocates nothing. No public type contains a
//! `Vec` or a `String`: responses borrow from a caller-supplied receive buffer,
//! and a handler writes its response into a caller-supplied sink.
//!
//! Only the async drivers need `std`, and only because the first of them sits
//! on `tokio` and std sockets. That is a property of the driver, not of the
//! API: a bare-metal driver presents the same shape, so gaining one later is
//! additive rather than a breaking redesign. Designing for that from the start
//! is the point — alloc-freedom cannot be retrofitted into a published API.
//!
//! ## Status
//!
//! **Prototype.** Every crate in this stack flows through a requirements-then-
//! architecture process, with a prototype preceding both to retire technical
//! risk; this is that prototype. The public API is unstable, most bodies are
//! unimplemented, and nothing here should be read as evidence of what the
//! standard requires — only of what is buildable.
//!
//! Known gaps, and the distance from this design to the pre-prototype code, are
//! recorded in `ARCHITECTURE.md` §9.
//!
//! ## What this crate deliberately does not do
//!
//! It does not decode UDS messages — that is `uds_protocol` — and it does not
//! dispatch services, choose negative response codes, or know what a data
//! identifier is. Those are ISO 14229-1 clause 8.7 concerns and belong to
//! `uds_services`, which drives this crate rather than being called by it.
//!
//! The one exception is narrow and forced by the standard: clause 8 keys TCP
//! connection handling on two specific service identifiers. See
//! [`profile::service_ids`].

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

pub mod error;
pub mod mapping;
pub mod profile;
pub mod transport;

pub use error::{Error, Result};
pub use profile::Timing;
pub use transport::{DoIpTransport, TransportEvent};
