// Above the workspace standard, and specific to this crate: a wildcard arm is how a
// variant gets silently discarded on its way to the seam. Denying it means a match
// over an enum must say what it does with every case. Not in the workspace table
// because the other four crates have 46 wildcard arms between them.
#![deny(clippy::wildcard_enum_match_arm)]
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
//!   uds_services      the driver — owns Client/Server, declares UdsTransport
//!        ↓  UdsTransport
//!   uds_on_ip         ISO 14229-5 profile + DoIP mapping  ← this crate
//!        ↓
//!   simple_doip       ISO 13400-2
//! ```
//!
//! This crate is **wholly below** the session layer. It hosts no driver, calls
//! nothing upward, and knows nothing about services. `uds_services` owns the
//! `uds_session::Client` or `uds_session::Server` — `UDSS_LLR_0029` fixes the
//! role at creation, so a node acting as both holds two instances rather than
//! one session object — supplies its inputs, drains its actions, and calls this
//! crate through the trait it declares.
//!
//! That trait is [`uds_services::UdsTransport`], and
//! [`transport::DoIpTransport`] implements it. The dependency edge runs from
//! here to `uds_services` and not the other way, because `uds_services` never
//! names a transport (`UDSSVC_ARCH_0002`) — which is what keeps a binding
//! additive at the application.
//!
//! The event type is `uds_services::TransportEvent`, taken from there rather
//! than mirrored here. A mirror is two vocabularies for one seam; this crate
//! carried one for a day and the two had already diverged on whether an event
//! borrows the driver's buffer when they were compared.
//!
//! ## `no_std`, alloc-freedom, and no runtime
//!
//! The crate is `no_std` and allocates nothing: no public type contains a
//! `Vec` or a `String`, and an inbound message borrows the receive buffer.
//!
//! It is **async without naming a runtime**. An `async fn` implies neither an
//! executor nor `std`, but a runtime *dependency* would compromise the
//! `no_std` build — and a bare-metal AURIX `TC4x` target is a qualification
//! target. [`transport::DoIpTransport`] is therefore generic over a
//! [`simple_doip::service::DiagnosticEntity`], a trait `simple_doip` declares
//! with no I/O in it; the sockets, and whatever runtime drives them, are the
//! entity's.
//!
//! ## Status
//!
//! **Prototype.** The public API is unstable. The server role is implemented over
//! any `DiagnosticEntity`; the client role is not yet. Known gaps are recorded in
//! `ARCHITECTURE.md` §9, which ships with the package, and each one is also named
//! at the item it affects — see [`mapping::PERIODIC_RESPONSE_PAYLOAD_TYPE`]'s
//! unreachable payload type.
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
//! `profile::service_ids`.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

pub mod error;
pub mod mapping;
pub mod profile;
pub mod transport;

pub use error::Error;
pub use transport::DoIpTransport;

// `TransportEvent` is deliberately not re-exported. It is
// `uds_services::TransportEvent`, and a caller matching on one reaches for it
// where the trait that produces it lives; re-exporting would put the same type
// under two paths and invite the mirror back under a different name.
