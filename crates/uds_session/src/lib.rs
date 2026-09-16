//! ISO 14229-2 session layer services.
//!
//! A transport-agnostic session layer for UDS diagnostics, implemented as a sans-io state
//! machine: no clock is read, no transport is called, and no executor is involved. The
//! caller supplies a monotonic timestamp together with inbound transport events, and
//! drains the resulting actions.
//!
//! The crate is `no_std` and performs no allocation. Storage is provided by the caller, so
//! sizing is a deployment decision rather than a compile-time constant of this crate.
//!
//! `unsafe` is forbidden crate-wide, and the lint configuration in `Cargo.toml` applies to
//! every target rather than to the crate root alone.
//!
//! # Status
//!
//! No behaviour is implemented yet. Requirements are authored before the code that
//! satisfies them — the ordering is evidence that cannot be reconstructed afterwards — so
//! implementation follows the published requirement set rather than preceding it.

#![no_std]

pub mod time;

pub use time::Timestamp;
