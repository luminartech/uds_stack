// A wildcard arm is how a variant gets silently discarded on its way to the seam, so a
// match over an enum here says what it does with every case. The other crates of the
// workspace carry wildcard arms, so the lint is this crate's rather than the table's.
#![deny(clippy::wildcard_enum_match_arm)]
//! # UDS on Internet Protocol
//!
//! An implementation of **ISO 14229-5:2022 (`UDSonIP`)**, the application profile
//! that binds Unified Diagnostic Services to a `DoIP` transport. A table, figure,
//! clause or requirement cited in this crate with no document named is
//! ISO 14229-5:2022's; any other document is named where it is cited.
//!
//! ## What this crate is
//!
//! ISO 14229-5 clause 7 enumerates its content as a table of requirements grouped
//! by OSI layer, and that table is this crate's scope. It owns two layers:
//!
//! - **Clause 8, the application profile** (`REQ 7.1`–`7.20`): the `A_PDU` format,
//!   TCP connection handling around `DiagnosticSessionControl` and `ECUReset`,
//!   periodic responses, and which timing parameters apply.
//! - **Clause 11, the transport mapping** (`REQ 4.3`, `REQ 4.4`): the `T_PDU`
//!   service primitives and parameters onto `DoIP`'s.
//!
//! Of a UDS message it reads four things, each required by the standard: whether a
//! response is a positive `DiagnosticSessionControl` or a positive `ECUReset`,
//! which clause 8 keys TCP connection handling on, and a request's suppress bit and
//! a response-pending, which decide whether an answer is owed
//! (`profile::service_ids`). Decoding the rest is `uds_protocol`'s; dispatch,
//! negative response codes and data identifiers are `uds_services`'s
//! (ISO 14229-1:2020 clause 8.7). Addressing, the service primitives and the
//! session state machine are `uds_session`'s, used from there rather than
//! redeclared here.
//!
//! ## Where it sits
//!
//! ```text
//!   consuming application
//!        ↕  typed service traits / typed client calls
//!   uds_services      the driver: owns Client/Server, declares UdsTransport
//!        ↓  UdsTransport
//!   uds_on_ip         ISO 14229-5 profile + DoIP mapping  ← this crate
//!        ↓
//!   simple_doip       ISO 13400-2
//! ```
//!
//! This crate is **wholly below** the session layer. It hosts no driver, calls
//! nothing upward, and knows nothing about services. `uds_services` owns the
//! `uds_session::Client` or `uds_session::Server` (`UDSS_LLR_0029` fixes the role at
//! creation, so a node acting as both holds two instances), supplies its inputs,
//! drains its actions, and calls this crate through [`uds_services::UdsTransport`], which
//! [`transport::DoIpTransport`] implements for a server over a `DoIP` entity and
//! [`client::DoIpClientTransport`] for a client over a tester connection, with
//! [`uds_services::ClientTransport`] beside it. The dependency edge runs from here
//! to `uds_services` and not the other way: `uds_services` never names a transport
//! (`UDSSVC_ARCH_0002`), which is what keeps a binding additive at the application.
//! The event type is `uds_services::TransportEvent`, taken from there rather than
//! mirrored here: a mirror is two vocabularies for one seam.
//!
//! ## `no_std`, alloc-freedom, and no runtime
//!
//! The crate is `no_std` and allocates nothing: no public type contains a `Vec` or
//! a `String`, and an inbound message borrows the receive buffer. It is async
//! without naming a runtime: [`transport::DoIpTransport`] is generic over a
//! [`simple_doip::service::DiagnosticEntity`], a trait with no I/O in it, and the
//! sockets, and whatever runtime drives them, are the entity's. A runtime
//! dependency would compromise the `no_std` build, and a bare-metal AURIX `TC4x`
//! is a qualification target.
//!
//! ## Status
//!
//! **Alpha.** The public API is unstable. The server role is implemented over any
//! `DiagnosticEntity`, and the client role over any `TesterConnection`; fanning a
//! functional request out to several entities is not. Known gaps are recorded in
//! `ARCHITECTURE.md` §6, which ships with the package, and each one is also named
//! at the item it affects; see [`mapping::PERIODIC_RESPONSE_PAYLOAD_TYPE`], a
//! payload type a server built on this crate cannot send.

#![no_std]
#![forbid(unsafe_code)]

pub mod client;
pub mod error;
pub mod mapping;
pub mod profile;
pub mod transport;

pub use client::{DoIpClientTransport, KEEP_ALIVE_LEN, LateReplyGuard};
pub use error::{DoIpClientTransportError, DoIpTransportError};
pub use transport::DoIpTransport;

// `TransportEvent` is not re-exported: it is `uds_services::TransportEvent`, and a
// caller matching on one reaches for it where the trait that produces it lives.
