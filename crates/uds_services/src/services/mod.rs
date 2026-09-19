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
//! **Every service trait declares `MAY_RESPOND_PENDING` with no default**, so omitting it
//! fails to compile. A default of `false` would let a forgotten declaration silently mean
//! "never 0x78", which is a conformance decision nobody made.

use crate::storage::Storage;
use crate::{Ai, ResponseSink};
use uds_protocol::NegativeResponseCode;

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

/// Whether clause 8.7 requires a response to be transmitted.
///
/// ``UDSSVC_ARCH_0016``.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Responded {
    /// Bytes were written and are to be transmitted.
    Yes,
    /// Clause 8.7 requires no response — a suppressed positive response, or a
    /// functionally addressed request whose code is one of the five
    /// ``UDSSVC_ARCH_0009`` silences.
    Suppressed,
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

    /// Run the clause 8.7 pipeline for `request` and write any response into `out`.
    ///
    /// ``UDSSVC_ARCH_0004`` — a function of the request and its addressing, with no
    /// transport, clock or session layer involved.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] clause 8.7 selected.
    fn dispatch(
        &mut self,
        request: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<Responded, NegativeResponseCode>>;

    /// Whether this server implements `sid` at all.
    ///
    /// ``UDSSVC_ARCH_0006`` — Figure 5's first mandatory check, and the one only the
    /// assembly list can answer.
    fn supports(&self, sid: u8) -> bool;

    /// Whether `sid`'s handler may answer `requestCorrectlyReceivedResponsePending`.
    ///
    /// ``UDSSVC_ARCH_0033``.
    fn may_respond_pending(&self, sid: u8) -> bool;

    /// Whether `request`, addressed this way, is one of clause 8.7.6's two exceptions to
    /// one-request-at-a-time.
    ///
    /// A **functionally addressed** `TesterPresent` with `suppressPosRspMsgIndication`
    /// set, which bypasses the occupied diagnostic protocol instance; or a request in the
    /// `0x00`–`0x0F` range, which aborts the active service. The second is predicated on
    /// "if a server supports services in the range of 0x00 to 0x0F", which only the
    /// assembly list knows — so classification is not merely arguably this crate's, it is
    /// available nowhere else.
    ///
    /// The addressing is a parameter because the first exception is conditioned on it and
    /// the bytes do not carry it. `ai` is the one the driver drained from the transport
    /// event alongside `request`.
    ///
    /// **The second limb is unreachable as the crate stands.** No service
    /// [`crate::uds_server`] can assemble falls in `0x00`–`0x0F` — that range is OBD
    /// territory, which `uds_protocol` does not model — so no assembled server can
    /// return `true` from it. The arm is kept because it is where the case will be
    /// handled when a service in that range arrives.
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
        assert_ne!(Responded::Yes, Responded::Suppressed);
    }
}
