//! The traits an application implements.
//!
//! ``UDSSVC_ARCH_0012``: each service is its own trait, so the assembly in
//! [`crate::uds_server`] is the list `serviceNotSupported` (0x11) is decided from. Every
//! handler returns `Result<(), NegativeResponseCode>`, `uds_protocol`'s code, and writes
//! into a [`ResponseSink`].
//!
//! A service trait whose handler can be in progress declares `MAY_RESPOND_PENDING` with
//! no default, so omitting it fails to compile. [`DiagnosticSessionControl`] declares it
//! for its `prepare`. [`TesterPresent`] does not: it is never awaited, so no
//! response-pending can come due.

use crate::pipeline::Settling;
use crate::state::ProtocolState;
use crate::storage::Storage;
use crate::{Ai, ResponseSink};
use uds_protocol::{
    DiagnosticSessionType, NegativeResponseCode, ResetType, UdsServiceType,
};

pub mod data;
pub mod dtc;
pub mod routine;
pub mod security;
pub mod session;
pub mod transfer;

pub use data::{ReadDataByIdentifier, WriteDataByIdentifier};
pub use dtc::{ClearDiagnosticInformation, DtcReportKind, ReadDtcInformation};
pub use routine::RoutineControl;
pub use security::{Delay, KeyVerdict, SecurityAccess, SecurityLevel, SecurityPolicy};
pub use session::{
    AfterReset, CommunicationControl, ControlDtcSetting, DiagnosticSessionControl,
    EcuReset, SessionTiming, TesterPresent,
};
pub use transfer::{DataTransfer, TransferRequest};

/// What a positive outcome does once its response is sent, or its suppression
/// completed: nothing, a change of session, or a reset (``UDSS_LLR_0085``/``0086``).
///
/// Public only because [`Responded`] names it; an application never builds or reads one.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Nothing follows the response.
    None,
    /// The response selected this session.
    Session(DiagnosticSessionType),
    /// An accepted `ECUReset`, performed on the confirmation (``UDSSVC_ARCH_0045``).
    Reset {
        /// The reset accepted.
        kind: ResetType,
        /// What follows it.
        after: AfterReset,
    },
}

impl From<Option<DiagnosticSessionType>> for Effect {
    fn from(selected: Option<DiagnosticSessionType>) -> Self {
        selected.map_or(Self::None, Self::Session)
    }
}

/// What clause 8.7 decided, and which effect the response carries.
///
/// ``UDSSVC_ARCH_0016``: a negative response is written bytes and arrives as
/// [`Responded::Yes`]; silence is the one outcome that is a reason not to transmit. The
/// effect rides on both arms because a suppressed `DiagnosticSessionControl` still
/// changes session, through the completion report (``UDSS_LLR_0086``, ``UDSS_LLR_0098``).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Responded {
    /// Bytes were written and are to be transmitted.
    Yes {
        /// What follows the response once it is confirmed sent.
        effect: Effect,
    },
    /// Clause 8.7 requires no response; the request is nonetheless complete, and the
    /// driver reports its completion to the session layer (``UDSS_LLR_0074``).
    Suppressed {
        /// What follows the completion report.
        effect: Effect,
    },
}

impl Responded {
    /// The effect, whichever arm.
    #[must_use]
    pub const fn effect(self) -> Effect {
        match self {
            Self::Yes { effect } | Self::Suppressed { effect } => effect,
        }
    }
}

/// What a request's handler left, before clause 8.7's last stages have run.
///
/// ``UDSSVC_ARCH_0016``: [`ServiceSet::dispatch`] returns this, and the driver hands it
/// to the pipeline's `settle`, which applies "response fits" (``UDSSVC_ARCH_0017``) and
/// ``UDSSVC_ARCH_0009``'s three suppression rules and yields the [`Responded`] the driver
/// acts on. The driver settles because rule 3's input, whether a 0x78 for this request
/// was accepted for transmission, is final only once the handler has finished.
///
/// Public only because the assembly's trait names it; an application never builds or
/// reads one. It records which of three shapes the request took: no service identifier
/// (``UDSSVC_ARCH_0005``), a code the common stages settled, or a service stage's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsettled(Shape);

/// The three ways a request reaches settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// No service identifier at all (``UDSSVC_ARCH_0005``): complete, with no response,
    /// whatever else is true.
    Empty,
    /// The common stages settled a code before any handler ran; its request carries no
    /// suppress bit the pipeline reads.
    Refused {
        /// `SIDRQ`, echoed by the negative response.
        sid: u8,
        /// The code the common stages settled.
        nrc: NegativeResponseCode,
    },
    /// The service's own stage ran.
    Handled {
        /// The service byte and the suppress bit.
        settling: Settling,
        /// What the stage returned: its effect, or a code.
        outcome: Result<Effect, NegativeResponseCode>,
    },
}

impl Unsettled {
    /// A request with no service identifier: `settle` reports it [`Responded::Suppressed`]
    /// with no effect, as it does a link closed mid-handler.
    #[doc(hidden)]
    #[must_use]
    pub const fn empty() -> Self {
        Self(Shape::Empty)
    }

    /// A request the common stages of `pipeline::begin` settled with `nrc`.
    #[doc(hidden)]
    #[must_use]
    pub const fn refused(sid: u8, nrc: NegativeResponseCode) -> Self {
        Self(Shape::Refused { sid, nrc })
    }

    /// A request whose service stage returned `outcome`.
    #[doc(hidden)]
    #[must_use]
    pub const fn handled(
        settling: Settling,
        outcome: Result<Effect, NegativeResponseCode>,
    ) -> Self {
        Self(Shape::Handled { settling, outcome })
    }

    /// What `settle` reads: `None` for [`Self::empty`], otherwise the request's
    /// [`Settling`] and its outcome, a refusal being an `Err` with no suppress bit.
    pub(crate) const fn parts(
        self,
    ) -> Option<(Settling, Result<Effect, NegativeResponseCode>)> {
        match self.0 {
            Shape::Empty => None,
            Shape::Refused { sid, nrc } => Some((
                Settling {
                    sid,
                    suppress_bit: false,
                },
                Err(nrc),
            )),
            Shape::Handled { settling, outcome } => Some((settling, outcome)),
        }
    }
}

/// A change of diagnostic session, classified.
///
/// ``UDSSVC_ARCH_0038``: ISO 14229-1:2020 clause 10.2 Figure 7's key names these four
/// cases, and which occurred determines what configuration state resets.
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

/// A request as the transport delivered it: whole, or only the front of a message longer
/// than the buffer it was received into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Received<'a> {
    /// The whole request, service identifier first.
    Whole(&'a [u8]),
    /// What fit of a request longer than the in-flight buffer, service identifier first.
    ///
    /// [`crate::uds_server`] sizes that buffer to the longest request any assembled service
    /// accepts, so such a request is never processed: it is checked as clause 8.7 checks
    /// any request, and one that would pass every check before the length is refused
    /// `incorrectMessageLengthOrInvalidFormat` (0x13).
    Truncated(&'a [u8]),
}

impl<'a> Received<'a> {
    /// The bytes received, service identifier first.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        match self {
            Self::Whole(bytes) | Self::Truncated(bytes) => bytes,
        }
    }
}

/// One application's assembled service implementations.
///
/// ``UDSSVC_ARCH_0013``: implemented by [`crate::uds_server`], never by hand, on the
/// marker type it generates in the invoking crate. The application, [`Self::App`], may
/// live in any crate. Sealed through [`crate::sealed`], so that [`Self::Store`]'s lengths
/// are always the ones the macro folded from this application's declared maxima.
pub trait ServiceSet: crate::sealed::Sealed {
    /// The application whose service traits this assembly calls.
    #[doc(hidden)]
    type App;

    /// The storage whose sizes were derived from this application's declared maxima.
    #[doc(hidden)]
    type Store: Storage;

    /// The protocol state this assembly keeps. ``UDSSVC_ARCH_0035``.
    #[doc(hidden)]
    type State: ProtocolState;

    /// Run the clause 8.7 pipeline for `request` up to its last stages, writing any
    /// response into `out`, and return the handler's outcome [`Unsettled`].
    ///
    /// ``UDSSVC_ARCH_0004``: a function of the request, its addressing
    /// (``UDSSVC_ARCH_0015``) and this crate's state, with no transport, clock or session
    /// layer involved. ``UDSSVC_ARCH_0016``: the driver, not this future, settles the
    /// result; see [`Unsettled`].
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] whose handlers answer, held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`]
    /// * `ai` - the [`Ai`] the driver drained with the request
    /// * `request` - the request, [`Received::Whole`] or [`Received::Truncated`]; only a
    ///   whole one reaches a handler
    /// * `out` - the [`ResponseSink`] the response is written into, and that the driver
    ///   settles once this future completes
    #[doc(hidden)]
    fn dispatch(
        app: &mut Self::App,
        state: &mut Self::State,
        ai: Ai,
        request: Received<'_>,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Unsettled>;

    /// The server is starting: what Annex I transition 1 owes at start-up.
    ///
    /// [`crate::Server`] calls it on its first step and on the first step after a
    /// [`Stopped`](crate::Stopped), since [`Server::new`](crate::Server::new) is `const`.
    /// Where `SecurityAccess` is listed, it starts the delay of every level whose stored
    /// attempt count is at its limit (see [`crate::SecurityAccess::delay`]); otherwise it
    /// does nothing.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`], held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`]
    #[doc(hidden)]
    fn start_up(app: &mut Self::App, state: &mut Self::State);

    /// `tS3_Server` expired (``UDSS_LLR_0100``): return to the default session and tell
    /// the application through `DiagnosticSessionControl::on_transition`
    /// (``UDSSVC_ARCH_0038``), where that service is listed.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] told of the transition, held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`],
    ///   whose session returns to
    ///   [`DefaultSession`](DiagnosticSessionType::DefaultSession)
    #[doc(hidden)]
    fn session_timed_out(app: &mut Self::App, state: &mut Self::State);

    /// A response selecting `selected` was confirmed sent (``UDSS_LLR_0085``/``0086``):
    /// enter it and tell the application (``UDSSVC_ARCH_0038``), as
    /// [`Self::session_timed_out`] does.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] told of the transition, held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`],
    ///   whose session becomes `selected`
    /// * `selected` - the [`DiagnosticSessionType`] the confirmed response selected
    #[doc(hidden)]
    fn session_confirmed(
        app: &mut Self::App,
        state: &mut Self::State,
        selected: DiagnosticSessionType,
    );

    /// The response accepting the reset `kind` was confirmed sent, or its suppression
    /// completed (``UDSSVC_ARCH_0045``): tell the application, and for
    /// [`AfterReset::Restart`] return to the default session (telling the application,
    /// Figure 7 key 4) and then to [`ProtocolState::INITIAL`]. Called only where
    /// `EcuReset` is listed.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] told of the reset, held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`]
    /// * `kind` - the reset accepted; see [`ResetType`]
    /// * `after` - what [`crate::EcuReset::reset`] said follows it
    #[doc(hidden)]
    fn reset_confirmed(
        app: &mut Self::App,
        state: &mut Self::State,
        kind: ResetType,
        after: AfterReset,
    );

    /// The change `effect`, accepted by its handler, is not made: tell the application
    /// through `DiagnosticSessionControl::abandon` or `EcuReset::abandon`, where the
    /// service is listed.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] told of the change, held by [`crate::Server`]
    /// * `state` - the [`ProtocolState`] of this assembly, held by [`crate::Server`],
    ///   whose session stays as it is
    /// * `effect` - the [`Effect`] that is not made
    #[doc(hidden)]
    fn abandoned(app: &mut Self::App, state: &Self::State, effect: Effect);

    /// The `P2` pair of the session `state` is in, which [`crate::Server`] enforces for
    /// the next request.
    ///
    /// [`DiagnosticSessionControl::timing`] for that session, or `None` where the
    /// assembly has no `DiagnosticSessionControl`: such a server never leaves the default
    /// session, and the [`ServerParams`](crate::ServerParams) it was built with time it.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] that states each session's timing
    /// * `state` - the [`ProtocolState`] of this assembly, which holds the session in force
    #[doc(hidden)]
    fn session_timing(app: &Self::App, state: &Self::State) -> Option<SessionTiming>;

    /// Whether entering `session` leaves the software this server is running, which
    /// [`crate::Server`] tells the transport with the response that selects it.
    ///
    /// [`DiagnosticSessionControl::leaves_running_software`], or `false` where the
    /// assembly has no `DiagnosticSessionControl` and so never changes session.
    ///
    /// # Arguments
    ///
    /// * `app` - the [`Self::App`] that knows which sessions leave its software
    /// * `session` - the session a positive response selects; see
    ///   [`DiagnosticSessionType`].
    #[doc(hidden)]
    fn leaves_running_software(app: &Self::App, session: DiagnosticSessionType) -> bool;

    /// Whether the assembly lists `service`. A byte that names no service arrives as
    /// [`UdsServiceType::UnsupportedDiagnosticService`], which no assembly list contains.
    ///
    /// ``UDSSVC_ARCH_0006``: Figure 5's first mandatory check refuses a service this does
    /// not list, and a listed one the assembly's `serves` (see [`crate::uds_server`])
    /// declines, so this alone does not predict `serviceNotSupported` (0x11).
    #[doc(hidden)]
    fn supports(service: UdsServiceType) -> bool;

    /// Whether `service`'s handler may answer `requestCorrectlyReceivedResponsePending`.
    ///
    /// ``UDSSVC_ARCH_0033``.
    #[doc(hidden)]
    fn may_respond_pending(service: UdsServiceType) -> bool;

    /// Whether `request`, addressed `ai`, may proceed while a service is already in
    /// progress: true for exactly the **functionally addressed** `TesterPresent` `3E 80`,
    /// the "valid `TesterPresent` message with SPRMIB=true" that ISO 14229-1:2020 8.7.6
    /// lets bypass the occupied diagnostic protocol instance. [`crate::Server`] answers
    /// anything else arriving mid-service `busyRepeatRequest` (0x21).
    ///
    /// Clause 8.7.6's second exception, a request in `0x00`-`0x0F` that aborts the active
    /// service, cannot arise: that range is OBD, which `uds_protocol` does not model, so
    /// [`crate::uds_server`] assembles no service in it.
    ///
    /// An associated function, because the driver asks while a handler holds the services.
    #[doc(hidden)]
    fn is_concurrent_exception(request: &[u8], ai: Ai) -> bool;
}

#[cfg(test)]
mod tests {
    use super::{Effect, Responded};

    /// ``UDSSVC_ARCH_0016``: silence is distinguishable from an empty response, because
    /// only one of them is a reason not to transmit.
    #[test]
    fn silence_and_an_empty_response_are_different_outcomes() {
        assert_ne!(
            Responded::Yes {
                effect: Effect::None
            },
            Responded::Suppressed {
                effect: Effect::None
            }
        );
    }
}
