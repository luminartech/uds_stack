//! # `uds_services`
//!
//! The point where an application meets a diagnostic stack, in both
//! directions: the interface by which a server integrates the stack, and the
//! set of requests available to a client application. Everything below deals
//! in bytes; this is the last layer that understands UDS.
//!
//! The standard behaviour it implements is ISO 14229-1:2020 clause 8.7, server
//! response implementation rules. That is a narrower claim than the paragraph
//! above, and the architecture keeps the two apart.
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
//! **Pre-implementation.** No behaviour exists yet. The architecture, and the
//! open questions that must be settled before the shape is fixed, are the
//! sphinx-needs set under `docs/architecture/`; `docs/design.md` is the earlier
//! design conversation, superseded in part.
//!
//! # Scope
//!
//! Clause 8.7 is what is implemented; being the stack's integration surface is
//! what the crate is. Message encode/decode is `uds_protocol`;
//! timers and response-pending are `uds_session`; framing and transport are
//! the binding's. A handler that knows how to fetch a data identifier has
//! nothing to say about IP, so the transport binding is an optional feature
//! and the same typed server works over `DoIP` or CAN.
//!
//! # Design constraints
//!
//! `no_std` and allocation-free. A handler writes its response into a
//! caller-supplied [`embedded_io::Write`] sink rather than returning a `Vec`,
//! and no public type carries a `Vec` or a `String`. This is designed in
//! rather than deferred: the signatures that make an API alloc-free are the
//! ones callers depend on, so it cannot be retrofitted later.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
