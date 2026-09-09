//! The request-handler seam.
//!
//! The server *driver* is not implemented in this prototype. What is defined
//! here is the interface `uds_services` will implement, so that crate can be
//! designed against a fixed target rather than a moving one.
//!
//! This module is `no_std` and alloc-free, because `uds_services` must be able
//! to sit on a bare-metal target.
//!
//! # The layering rule this encodes
//!
//! `uds_on_ip` hands a request to a handler as **bytes plus context**, and the
//! handler writes its response bytes back through a sink. It does not know what
//! a service is, what a data identifier is, or which negative response code
//! applies — those are ISO 14229-1 clause 8.7 concerns, and clause 8.7 is not
//! in ISO 14229-5's scope table.
//!
//! `uds_services` will provide a blanket implementation of [`RequestHandler`]
//! for any typed server, behind a feature gate, so neither crate need depend on
//! the other unconditionally. See `ARCHITECTURE.md` §4.3.

use crate::addressing::Ai;

/// State a handler needs in order to apply ISO 14229-1 clause 8.7's session and
/// security gating.
///
/// Every field is passed through uninterpreted. `uds_on_ip` learns the active
/// session and security level from the session layer and forwards them; it does
/// not decide what they permit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ctx {
    /// Who sent the request, and how it was addressed.
    ///
    /// The addressing mode matters to clause 8.7: a functionally addressed
    /// request that a server does not support is answered with silence rather
    /// than a negative response.
    pub ai: Ai,
    /// The active diagnostic session, as its ISO 14229-1 sub-function value.
    ///
    /// A raw `u8` deliberately: naming the sessions would mean knowing what
    /// `DiagnosticSessionControl`'s sub-functions mean, which is the layering
    /// violation this seam exists to prevent.
    pub active_session: u8,
    /// The active security level, as its ISO 14229-1 sub-function value.
    pub security_level: u8,
}

/// What a handler decided to do with a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// A response was written to the sink and should be transmitted.
    Responded,
    /// Send nothing.
    ///
    /// Required by ISO 14229-1 clause 8.7 for a suppressed positive response,
    /// and for a functionally addressed request the server does not support.
    Suppress,
}

/// Handles UDS requests arriving over `DoIP`.
///
/// Implemented by `uds_services` for typed servers; implementable directly by
/// an application that would rather work in bytes.
///
/// # Why the sink is generic rather than `dyn`
///
/// A response is written into a sink the caller owns, so no allocation is
/// needed to return one. Making the sink a generic parameter costs object
/// safety — there is no `Box<dyn RequestHandler>` — which is the right trade
/// for a crate that must build without `alloc`, where boxing is unavailable
/// anyway. A server driver is generic over `H: RequestHandler` instead.
pub trait RequestHandler {
    /// Handle one request, writing any response into `out`.
    ///
    /// Returning an [`Outcome`] rather than a response value is deliberate: a
    /// negative response is a normal outcome expressed in the written bytes,
    /// not an error. Genuine transport failures are this crate's concern and
    /// never reach a handler.
    ///
    /// # Errors
    /// Only sink failures. Protocol-level rejection is an `Outcome`, not an
    /// error.
    fn handle<W>(&mut self, ctx: &Ctx, request: &[u8], out: &mut W)
    -> Result<Outcome, W::Error>
    where
        W: embedded_io::Write;
}
