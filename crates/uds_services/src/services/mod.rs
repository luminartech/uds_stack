//! The traits an application implements.
//!
//! ``UDSSVC_ARCH_0012`` — each service is its own trait, so the assembly in
//! [`crate::uds_server`] knows exactly which a server supports. A single large trait with
//! defaulted methods would make "supported" mean "did not override", which is not a
//! distinction clause 8.7 can act on: `serviceNotSupported` (0x11) is decided from a
//! list, and the assembly is that list.
//!
//! Every handler returns `Result<(), NegativeResponseCode>` and writes into a
//! [`ResponseSink`]. The negative response code is `uds_protocol`'s;
//! this crate defines none.
//!
//! **A service trait whose handler can be in progress declares `MAY_RESPOND_PENDING`
//! with no default**, so omitting it fails to compile. A default of `false` would let a
//! forgotten declaration silently mean "never 0x78", which is a conformance decision
//! nobody made. [`DiagnosticSessionControl`] and [`TesterPresent`] do not declare it:
//! neither is ever awaited, so no response-pending can come due and the constant would
//! have had one possible value and no effect.

use crate::state::ProtocolState;
use crate::storage::Storage;
use crate::{Ai, ResponseSink};
use uds_protocol::{DiagnosticSessionType, UdsServiceType};

pub mod data;
pub mod dtc;
pub mod routine;
pub mod security;
pub mod session;
pub mod transfer;

pub use data::{ReadDataByIdentifier, WriteDataByIdentifier};
pub use dtc::{ClearDiagnosticInformation, DtcReportKind, ReadDtcInformation};
pub use routine::RoutineControl;
pub use security::{KeyVerdict, SecurityAccess, SecurityLevel, SecurityPolicy};
pub use session::{
    CommunicationControl, ControlDtcSetting, DiagnosticSessionControl, EcuReset,
    SessionTiming, TesterPresent,
};
pub use transfer::{DataTransfer, TransferRequest};

/// What clause 8.7 decided, and which session the response selected.
///
/// ``UDSSVC_ARCH_0016`` — a negative response is written bytes and arrives as
/// [`Responded::Yes`]; silence is a distinguishable outcome because only it is a reason
/// not to transmit. The session rides on both arms because a suppressed
/// `DiagnosticSessionControl` still changes session, through the completion report
/// (``UDSS_LLR_0086``, ``UDSS_LLR_0098``).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Responded {
    /// Bytes were written and are to be transmitted.
    Yes {
        /// The session a positive `DiagnosticSessionControl` response selected.
        session: Option<DiagnosticSessionType>,
    },
    /// Clause 8.7 requires no response; the request is nonetheless complete, and the
    /// driver reports its completion to the session layer (``UDSS_LLR_0074``).
    Suppressed {
        /// The session a suppressed `DiagnosticSessionControl` selected.
        session: Option<DiagnosticSessionType>,
    },
}

impl Responded {
    /// The selected session, whichever arm.
    #[must_use]
    pub const fn session(self) -> Option<DiagnosticSessionType> {
        match self {
            Self::Yes { session } | Self::Suppressed { session } => session,
        }
    }
}

/// A change of diagnostic session, classified.
///
/// ``UDSSVC_ARCH_0038`` — ISO 14229-1:2020 clause 10.2 Figure 7's key names these four
/// cases, and which occurred determines what configuration state resets. Classifying it
/// here is what stops every application reimplementing Table 23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionTransition {
    /// Default to default: a re-entry.
    DefaultToDefault,
    /// Default to a non-default session.
    DefaultToNonDefault,
    /// One non-default session to another.
    NonDefaultToNonDefault,
    /// A non-default session to default, by request or by `tS3_Server` expiry.
    NonDefaultToDefault,
}

impl SessionTransition {
    /// Which of Figure 7's four transitions `from` → `to` is.
    #[must_use]
    pub const fn classify(from: DiagnosticSessionType, to: DiagnosticSessionType) -> Self {
        let from_default = matches!(from, DiagnosticSessionType::DefaultSession);
        let to_default = matches!(to, DiagnosticSessionType::DefaultSession);
        match (from_default, to_default) {
            (true, true) => Self::DefaultToDefault,
            (true, false) => Self::DefaultToNonDefault,
            (false, false) => Self::NonDefaultToNonDefault,
            (false, true) => Self::NonDefaultToDefault,
        }
    }
}

/// One application's assembled service implementations.
///
/// ``UDSSVC_ARCH_0013`` — implemented by [`crate::uds_server`], never by hand.
///
/// Sealed through [`crate::sealed`]. The whole derivation argument — that
/// [`Self::Store`]'s lengths are folded from this application's declared maxima — holds
/// only while the macro is what chooses them; a hand-written impl can pick any lengths it
/// likes and still advertise the declared maxima on the wire.
pub trait ServiceSet: crate::sealed::Sealed {
    /// The storage whose sizes were derived from this application's declared maxima.
    type Store: Storage;

    /// The protocol state this assembly keeps. ``UDSSVC_ARCH_0035``.
    type State: ProtocolState;

    /// Run the clause 8.7 pipeline for `request` and write any response into `out`.
    ///
    /// ``UDSSVC_ARCH_0004`` — a function of the request, its addressing and this crate's
    /// state, with no transport, clock or session layer involved. ``UDSSVC_ARCH_0015`` is
    /// why `ai` is here. `pending_sent` is ``UDSSVC_ARCH_0009`` rule 3's input: whether a
    /// `requestCorrectlyReceivedResponsePending` already went out for this request, in
    /// which case nothing suppresses the final response. A shared reference, because the
    /// driver learns that while this future is live and a shared reference is the one
    /// thing that can coexist with it; an [`AtomicBool`](core::sync::atomic::AtomicBool)
    /// rather than a `Cell`, because it must be `Sync` for the driver's `step` future to
    /// stay `Send`.
    ///
    /// # Arguments
    ///
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`]
    /// * `ai` - the [`Ai`] the driver drained with the request
    /// * `request` - the request bytes, service identifier first
    /// * `out` - the [`ResponseSink`] the response is written into
    /// * `pending_sent` - set by the driver when a 0x78 for this request is accepted
    ///   for transmission, possibly while this future is live; read at settlement
    fn dispatch(
        &mut self,
        state: &mut Self::State,
        ai: Ai,
        request: &[u8],
        out: &mut ResponseSink<'_>,
        pending_sent: &core::sync::atomic::AtomicBool,
    ) -> impl core::future::Future<Output = Responded>;

    /// `tS3_Server` expired: return to the default session and tell the application.
    ///
    /// ``UDSS_LLR_0100`` reports the expiry; ``UDSSVC_ARCH_0038`` has the application
    /// told through `DiagnosticSessionControl::on_transition`. Emitted by the assembly,
    /// because only it knows whether that service is implemented; a no-op where it is not.
    ///
    /// # Arguments
    ///
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`],
    ///   whose session returns to
    ///   [`DefaultSession`](DiagnosticSessionType::DefaultSession)
    fn session_timed_out(&mut self, state: &mut Self::State);

    /// A response selecting `selected` was confirmed sent: enter it and tell the
    /// application. ``UDSS_LLR_0085``/``0086`` is the moment; ``UDSSVC_ARCH_0038`` the
    /// call. Emitted by the assembly, as [`Self::session_timed_out`] is.
    ///
    /// # Arguments
    ///
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`],
    ///   whose session becomes `selected`
    /// * `selected` - the [`DiagnosticSessionType`] the confirmed response selected
    fn session_confirmed(
        &mut self,
        state: &mut Self::State,
        selected: DiagnosticSessionType,
    );

    /// Whether this server implements `service` at all.
    ///
    /// ``UDSSVC_ARCH_0006`` — Figure 5's first mandatory check, and the one only the
    /// assembly list can answer. A byte that names no service arrives as
    /// [`UdsServiceType::UnsupportedDiagnosticService`], which no assembly list contains,
    /// so it answers `false` without a special case.
    fn supports(&self, service: UdsServiceType) -> bool;

    /// Whether `service`'s handler may answer `requestCorrectlyReceivedResponsePending`.
    ///
    /// ``UDSSVC_ARCH_0033``.
    fn may_respond_pending(&self, service: UdsServiceType) -> bool;

    /// Whether `request`, addressed this way, may proceed while a service is already in
    /// progress.
    ///
    /// True for a **functionally addressed** `TesterPresent` carrying
    /// `suppressPosRspMsgIndication`, which clause 8.7.6 lets bypass the occupied
    /// diagnostic protocol instance. The addressing is a parameter because that
    /// condition turns on it and the bytes do not carry it; `ai` is the one the driver
    /// drained from the transport event alongside `request`.
    ///
    /// Clause 8.7.6 admits a second exception — a request in `0x00`–`0x0F`, which aborts
    /// the active service — and no assembled server can meet it. That range is OBD
    /// territory, which `uds_protocol` does not model, so [`crate::uds_server`] can
    /// assemble no service in it and the case cannot arise. Its absence here is coverage,
    /// not an omission.
    ///
    /// Anything else arriving mid-service is occupancy and owes `busyRepeatRequest`
    /// (0x21). *Acting* on a classification is open question 1.
    fn is_concurrent_exception(&self, request: &[u8], ai: Ai) -> bool;
}

#[cfg(test)]
mod tests {
    use super::Responded;

    /// ``UDSSVC_ARCH_0016`` — silence must be distinguishable from an empty response,
    /// because only one of them is a reason not to transmit. A `bool` would leave the
    /// driver guessing which, and this test is what fails if someone replaces it.
    #[test]
    fn silence_and_an_empty_response_are_different_outcomes() {
        assert_ne!(
            Responded::Yes { session: None },
            Responded::Suppressed { session: None }
        );
    }
}
