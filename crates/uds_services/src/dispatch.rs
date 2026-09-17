//! The clause 8.7 pipeline. Internal.
//!
//! ``UDSSVC_ARCH_0018`` — nothing outside this crate calls this. The driver passes the
//! addressing it drained and calls in; what is here is its own vocabulary, not a seam.
//!
//! **There is no `Ctx`.** ``UDSSVC_ARCH_0015`` carried active session, security level and
//! authentication state as fields "under review", and all three resolved the same way:
//! this crate implements `DiagnosticSessionControl`, `SecurityAccess` and
//! `Authentication`, so under ``UDSSVC_ARCH_0035`` it already holds them, and reading them
//! from a struct it just built is a copy rather than an input. Its closing question —
//! whether it stays a struct at all — answers itself. The driver passes
//! `uds_session::Ai`, which ISO 14229-1:2020 clause 7.4.1 makes a mandatory parameter of
//! every application layer service primitive anyway.
//!
//! ``UDSSVC_ARCH_0004`` — the pipeline is a function of the request and its addressing,
//! so every clause 8.7 rule is testable without a network, a clock or a session layer.

use uds_protocol::NegativeResponseCode;
use uds_session::{Ai, TaType};

/// What the pipeline decided.
///
/// ``UDSSVC_ARCH_0016`` — silence must be distinguishable from an empty response, because
/// the loop has to tell "clause 8.7 requires no response" from "the handler produced an
/// empty one", and only the first is a reason not to transmit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(
    dead_code,
    reason = "the pipeline that reads it is UDSSVC_ARCH_0042's pass"
)]
pub(crate) enum Outcome {
    /// Bytes were written and are to be transmitted.
    Responded,
    /// Clause 8.7 requires no response.
    Silent,
}

/// Whether `code` is silenced for a request addressed this way.
///
/// ``UDSSVC_ARCH_0009`` rule 1. ISO 14229-1:2020 clause 8.7.5 suppresses exactly five
/// negative response codes on a functionally addressed request, and Annex A.1 names them.
/// The list is closed: silencing any other would be a server that never reports the
/// failure it had.
#[allow(
    dead_code,
    reason = "no caller until the UDSSVC_ARCH_0042 pipeline lands"
)]
pub(crate) const fn suppresses(code: NegativeResponseCode, ai: Ai) -> bool {
    matches!(ai.ta_type, TaType::Functional)
        && matches!(
            code,
            NegativeResponseCode::ServiceNotSupported
                | NegativeResponseCode::SubFunctionNotSupported
                | NegativeResponseCode::ServiceNotSupportedInActiveSession
                | NegativeResponseCode::SubFunctionNotSupportedInActiveSession
                | NegativeResponseCode::RequestOutOfRange
        )
}

#[cfg(test)]
mod tests {
    use super::suppresses;
    use uds_protocol::NegativeResponseCode as N;
    use uds_session::{Address, Ai, Mtype, TaType};

    fn ai(ta_type: TaType) -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0E00),
            ta_type,
        }
    }

    /// ``UDSSVC_ARCH_0009`` rule 1 — clause 8.7.5 silences exactly five negative
    /// response codes on a functionally addressed request, and Annex A.1 names them.
    #[test]
    fn the_five_codes_are_silenced_only_when_functionally_addressed() {
        for code in [
            N::ServiceNotSupported,
            N::SubFunctionNotSupported,
            N::ServiceNotSupportedInActiveSession,
            N::SubFunctionNotSupportedInActiveSession,
            N::RequestOutOfRange,
        ] {
            assert!(
                suppresses(code, ai(TaType::Functional)),
                "{code:?} functional"
            );
            assert!(!suppresses(code, ai(TaType::Physical)), "{code:?} physical");
        }
    }

    /// The list is closed. Silencing any other code would be a server that never
    /// reports the failure it actually had.
    #[test]
    fn other_codes_are_never_silenced() {
        for code in [
            N::ConditionsNotCorrect,
            N::SecurityAccessDenied,
            N::ResponseTooLong,
        ] {
            assert!(!suppresses(code, ai(TaType::Functional)));
            assert!(!suppresses(code, ai(TaType::Physical)));
        }
    }
}
