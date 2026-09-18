//! # `uds_services`
//!
//! The point where an application meets a diagnostic stack, in both
//! directions: the interface by which a server integrates the stack, and the
//! set of requests available to a client application. Everything below deals
//! in bytes; this is the last layer that understands UDS.
//!
//! Its scope is ISO 14229-1:2020's *behaviour*: `uds_protocol` owns that
//! document's format — the bits, the bytes, and which messages are valid — and
//! this crate owns everything else in it. Clause 8.7, the server response
//! implementation rules, is the densest part of that and the reason the crate
//! exists, but it is not the boundary.
//!
//! A UDS server must do more than answer the requests it supports. Clause 8.7
//! specifies the whole validation sequence: which checks run in which order,
//! which negative response code each failure produces, and when the correct
//! answer is no response at all. This crate centralises those rules so an
//! application defines its identifiers, implements the services it supports,
//! and the rest follows.
//!
//! A client application names services in that same identifier vocabulary and
//! gets typed results back, negative responses included — which the layer
//! below explicitly declines to interpret.
//!
//! # Status
//!
//! **API stub.** The public surface is complete; behaviour is not. Every entry
//! point is `todo!()` and carries the architecture element it will satisfy.
//! `tests/composition.rs` assembles a server against the whole surface.
//!
//! # Scope
//!
//! ISO 14229-1's behaviour is what is implemented; being the stack's
//! integration surface is what the crate is. Message encode/decode is
//! `uds_protocol`; session timing is `uds_session`; framing and transport are
//! the binding's. Deciding to
//! answer `requestCorrectlyReceivedResponsePending` (0x78) is this crate's,
//! because ISO 14229-2 makes that decision turn on whether the server supports
//! the service; *when* one is due remains the session layer's.
//!
//! A handler that knows how to fetch a data identifier has nothing to say
//! about IP. A binding is never a feature of this crate — it implements
//! [`UdsTransport`], and the application names it (`UDSSVC_ARCH_0003`).
//!
//! # Design constraints
//!
//! `no_std` and allocation-free. A handler writes its response into a
//! [`ResponseSink`] this crate owns rather than returning a `Vec`,
//! and no public type carries a `Vec` or a `String`. This is designed in
//! rather than deferred: the signatures that make an API alloc-free are the
//! ones callers depend on, so it cannot be retrofitted later.

#![no_std]
#![forbid(unsafe_code)]

pub mod assembly;

pub mod client;
pub use client::{Answer, Client, Response, Responses};

pub mod server;
pub use server::Server;

pub mod storage;
pub use storage::{Buffers, Storage, Store};

pub mod sink;
pub use sink::ResponseSink;

pub mod transport;
pub use transport::{TransportEvent, UdsTransport};

pub mod select;
pub use select::{Either, Select2, select2};

pub mod identifier;
pub use identifier::{DataIdentifier, RecordError, RoutineIdentifier};

pub mod services;
pub use services::{
    ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer,
    DiagnosticSessionControl, EcuReset, KeyVerdict, ReadDataByIdentifier,
    ReadDtcInformation, Responded, RoutineControl, SecurityAccess, SecurityLevel,
    SecurityPolicy, ServiceSet, SessionTiming, SessionTransition, TesterPresent,
    TransferRequest, WriteDataByIdentifier,
};

#[doc(hidden)]
pub mod sealed;

mod dispatch;
