//! The clause 8.7 pipeline. ``UDSSVC_ARCH_0004`` — an ordered sequence of stages, each
//! settling the request or passing it on; ``UDSSVC_ARCH_0018`` — nothing outside this
//! crate calls it. Every item is `pub` only because `uds_server!` expands in the
//! application's crate and routes here; none is API an application writes against.
//!
//! **The pipeline takes no context struct.** Under ``UDSSVC_ARCH_0035`` this crate
//! already holds the active session, the security level and the authentication state,
//! because it implements the services that own them — so reading them back out of a
//! struct it had just built would be a copy, not an input. What the driver does pass is
//! `uds_session::Ai`, which ISO 14229-1:2020 clause 7.4.1 makes a mandatory parameter of
//! every application layer service primitive anyway.

use crate::services::SessionTransition;
use crate::state::State;
use crate::{Responded, ResponseSink};
use automotive_wire_codec::Sink;
use core::cell::Cell;
use uds_protocol::{
    Decode, DiagnosticSessionType, Encode, NegativeResponse, NegativeResponseCode, Request,
    UdsServiceType,
};
use uds_session::{Ai, TaType};

/// Enter `to`, and say which of Figure 7's transitions that was (``UDSSVC_ARCH_0038``).
/// The one place the session field is written; the macro's hooks call this and nothing
/// else, so `State`'s accessors stay crate-private and no clause 10.2 logic is emitted
/// into the application's crate.
#[doc(hidden)]
pub fn transition(state: &mut State, to: DiagnosticSessionType) -> SessionTransition {
    let from = state.session();
    state.set_session(to);
    SessionTransition::classify(from, to)
}

/// Placeholder until Task 12 routes `dispatch` here. ``UDSSVC_ARCH_0004``.
///
/// Not `async`, as the pipeline it stands in for is: it awaits nothing, and
/// `clippy::unused_async` refuses an `async fn` that does not.
#[doc(hidden)]
#[must_use]
pub const fn dispatch_stub(_request: &[u8], _out: &mut ResponseSink<'_>) -> Responded {
    Responded::Suppressed { session: None }
}

/// Where the common stages left the request.
#[doc(hidden)]
#[derive(Debug)]
pub enum Stage<'a> {
    /// Decoded, supported, allowed: the service's own stage runs next.
    Proceed {
        /// `SIDRQ` — the request's service identifier.
        sid: u8,
        /// The request as `uds_protocol` decoded it.
        request: Request<'a>,
    },
    /// Settled with a code; `settle` writes it.
    Settle {
        /// `SIDRQ` — the request's service identifier, echoed by the negative response.
        sid: u8,
        /// The code the request settled with.
        nrc: NegativeResponseCode,
    },
    /// No service identifier at all (spec §3.3): complete, with no response.
    Empty,
}

/// ``UDSSVC_ARCH_0005`` (decode → 0x13; unmodelled → 0x11) and ``UDSSVC_ARCH_0006``
/// checks 1 and 3. Check 2, authentication, is unconditionally true (architecture open
/// question 4); the security precondition (0x33) joins with security state.
#[doc(hidden)]
#[must_use]
pub fn begin<'a, F: Fn(UdsServiceType) -> bool>(
    state: &State,
    request: &'a [u8],
    supports: F,
) -> Stage<'a> {
    let Some((&sid, _)) = request.split_first() else {
        return Stage::Empty;
    };
    let Ok((decoded, [])) = Request::decode(request) else {
        return Stage::Settle {
            sid,
            nrc: NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
        };
    };
    let service = match decoded {
        Request::Other { .. } => UdsServiceType::UnsupportedDiagnosticService,
        ref other => other.service(),
    };
    if !supports(service) {
        return Stage::Settle {
            sid,
            nrc: NegativeResponseCode::ServiceNotSupported,
        };
    }
    if !allowed_in_session(service, state.session()) {
        return Stage::Settle {
            sid,
            nrc: NegativeResponseCode::ServiceNotSupportedInActiveSession,
        };
    }
    Stage::Proceed {
        sid,
        request: decoded,
    }
}

/// ISO 14229-1:2020 10.2 Table 23 — the twelve services "not applicable" in the default
/// session (``UDSSVC_ARCH_0006``). Everything else is allowed; the footnoted rows are the
/// application's and are not refused here.
#[doc(hidden)]
#[must_use]
pub const fn allowed_in_session(
    service: UdsServiceType,
    session: DiagnosticSessionType,
) -> bool {
    if !matches!(session, DiagnosticSessionType::DefaultSession) {
        return true;
    }
    !matches!(
        service,
        UdsServiceType::SecurityAccess
            | UdsServiceType::CommunicationControl
            | UdsServiceType::SecuredDataTransmission
            | UdsServiceType::ControlDtcSetting
            | UdsServiceType::LinkControl
            | UdsServiceType::ReadDataByIdentifierPeriodic
            | UdsServiceType::InputOutputControlByIdentifier
            | UdsServiceType::RequestDownload
            | UdsServiceType::RequestUpload
            | UdsServiceType::TransferData
            | UdsServiceType::RequestTransferExit
            | UdsServiceType::RequestFileTransfer
    )
}

/// Whether `code` is silenced for a request addressed this way.
///
/// ``UDSSVC_ARCH_0009`` rule 1. ISO 14229-1:2020 clause 8.7.5 suppresses exactly five
/// negative response codes on a functionally addressed request, and Annex A.1 names them.
/// The list is closed: silencing any other would be a server that never reports the
/// failure it had.
#[doc(hidden)]
#[must_use]
pub const fn suppresses(code: NegativeResponseCode, ai: Ai) -> bool {
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

/// What `settle` needs to know about the request: the service byte a negative response
/// echoes, and whether the request asked for its positive response to be suppressed.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Settling {
    /// `SIDRQ` — the request's service identifier, echoed by a negative response.
    pub sid: u8,
    /// `suppressPosRspMsgIndicationBit`, where the service has a sub-function byte.
    pub suppress_bit: bool,
}

/// The last two stages: "response fits" (``UDSSVC_ARCH_0017`` → 0x14) and the
/// suppression gate (``UDSSVC_ARCH_0009``). Writes a negative response itself; the
/// handler's bytes, if any, are discarded by rewinding. `pending_sent` is read here, at
/// the moment of settlement, which is what lets a 0x78 sent while the handler ran lift
/// both suppressions (rule 3).
#[doc(hidden)]
pub fn settle(
    ai: Ai,
    settling: Settling,
    pending_sent: &Cell<bool>,
    outcome: Result<Option<DiagnosticSessionType>, NegativeResponseCode>,
    out: &mut ResponseSink<'_>,
) -> Responded {
    let Settling { sid, suppress_bit } = settling;
    let pending_sent = pending_sent.get();
    let outcome = if out.refused() {
        Err(NegativeResponseCode::ResponseTooLong)
    } else {
        outcome
    };
    match outcome {
        Ok(session) => {
            // Rule 2: the bit silences positive responses; rule 3: not after a 0x78.
            if suppress_bit && !pending_sent {
                Responded::Suppressed { session }
            } else {
                Responded::Yes { session }
            }
        }
        Err(nrc) => {
            // Rule 1, overridden by rule 3.
            if suppresses(nrc, ai) && !pending_sent {
                return Responded::Suppressed { session: None };
            }
            out.rewind();
            // `NegativeResponse::encode` writes `[sid, nrc]`; the 0x7F service byte is
            // the pipeline's, as the 0x50/0x62/0x7E positive-response bytes are. Cannot
            // be refused: the sink's bound is never below three bytes (Task 8).
            let _ = out.write_all(&[0x7F]);
            let _ = NegativeResponse::new_with_sid(sid, nrc).encode(out);
            Responded::Yes { session: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Settling, Stage, allowed_in_session, begin, settle, suppresses};
    use crate::state::{ProtocolState, State};
    use crate::{Responded, ResponseSink};
    use automotive_wire_codec::Sink;
    use core::cell::Cell;
    use uds_protocol::NegativeResponseCode as N;
    use uds_protocol::{DiagnosticSessionType as S, UdsServiceType as U};
    use uds_session::{Address, Ai, Mtype, TaType};

    fn ai(ta_type: TaType) -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0E00),
            ta_type,
        }
    }

    fn sid(sid: u8) -> Settling {
        Settling {
            sid,
            suppress_bit: false,
        }
    }

    fn supports_rdbi(s: U) -> bool {
        matches!(s, U::ReadDataByIdentifier)
    }

    /// Spec §3.3 — an empty request is the pipeline's, and is `Empty`.
    #[test]
    fn an_empty_request_is_empty() {
        assert!(matches!(
            begin(&State::INITIAL, &[], supports_rdbi),
            Stage::Empty
        ));
    }

    /// ``UDSSVC_ARCH_0005`` — a decode failure settles 0x13 with the SID echoed.
    #[test]
    fn a_short_request_settles_0x13() {
        let st = begin(&State::INITIAL, &[0x22, 0xF1], supports_rdbi);
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x22,
                nrc: N::IncorrectMessageLengthOrInvalidFormat
            }
        ));
    }

    /// ``UDSSVC_ARCH_0005``, ``UDSSVC_ARCH_0006`` check 1 — an unmodelled or unlisted
    /// SID settles 0x11, not 0x13.
    #[test]
    fn an_unsupported_service_settles_0x11() {
        let st = begin(&State::INITIAL, &[0x3E, 0x00], supports_rdbi);
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x3E,
                nrc: N::ServiceNotSupported
            }
        ));
        let st = begin(&State::INITIAL, &[0xBA, 0x00], supports_rdbi);
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0xBA,
                nrc: N::ServiceNotSupported
            }
        ));
    }

    /// ``UDSSVC_ARCH_0006`` check 3 — Table 23's twelve "not applicable" rows are
    /// refused in the default session with 0x7F.
    #[test]
    fn table_23_services_are_refused_in_the_default_session() {
        assert!(!allowed_in_session(U::SecurityAccess, S::DefaultSession));
        assert!(allowed_in_session(
            U::SecurityAccess,
            S::ExtendedDiagnosticSession
        ));
        assert!(allowed_in_session(
            U::ReadDataByIdentifier,
            S::DefaultSession
        ));
        assert!(allowed_in_session(
            U::DiagnosticSessionControl,
            S::DefaultSession
        ));
        let mut state = State::INITIAL;
        state.set_session(S::DefaultSession);
        let st = begin(&state, &[0x27, 0x01], |s| matches!(s, U::SecurityAccess));
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x27,
                nrc: N::ServiceNotSupportedInActiveSession
            }
        ));
    }

    /// A well-formed, supported request proceeds with its decoded form.
    #[test]
    fn a_good_request_proceeds() {
        let st = begin(&State::INITIAL, &[0x22, 0xF1, 0x90], supports_rdbi);
        assert!(matches!(st, Stage::Proceed { sid: 0x22, .. }));
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

    /// `settle`: a negative outcome writes `7F sid nrc` over whatever the handler left.
    #[test]
    fn a_negative_outcome_replaces_the_partial_response() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let _ = out.write_all(&[0x62, 0xF1]);
        let no = Cell::new(false);
        let r = settle(
            ai(TaType::Physical),
            sid(0x22),
            &no,
            Err(N::RequestOutOfRange),
            &mut out,
        );
        assert_eq!(r, Responded::Yes { session: None });
        assert_eq!(out.written_bytes(), &[0x7F, 0x22, 0x31]);
    }

    /// `settle`: rule 1 silences; rule 3 (a sent 0x78) overrides it.
    #[test]
    fn functional_silence_is_overridden_by_a_sent_response_pending() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let no = Cell::new(false);
        let r = settle(
            ai(TaType::Functional),
            sid(0x22),
            &no,
            Err(N::RequestOutOfRange),
            &mut out,
        );
        assert_eq!(r, Responded::Suppressed { session: None });
        let yes = Cell::new(true);
        let r = settle(
            ai(TaType::Functional),
            sid(0x22),
            &yes,
            Err(N::RequestOutOfRange),
            &mut out,
        );
        assert_eq!(r, Responded::Yes { session: None });
    }

    /// `settle`: rule 2 — the suppress bit silences a positive response only.
    #[test]
    fn the_suppress_bit_silences_positive_responses_only() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let _ = out.write_all(&[0x50, 0x03, 0, 50, 1, 244]);
        let no = Cell::new(false);
        let bit = Settling {
            sid: 0x10,
            suppress_bit: true,
        };
        let r = settle(
            ai(TaType::Physical),
            bit,
            &no,
            Ok(Some(S::ExtendedDiagnosticSession)),
            &mut out,
        );
        assert_eq!(
            r,
            Responded::Suppressed {
                session: Some(S::ExtendedDiagnosticSession)
            }
        );
        let r = settle(
            ai(TaType::Physical),
            bit,
            &no,
            Err(N::SubFunctionNotSupported),
            &mut out,
        );
        assert_eq!(r, Responded::Yes { session: None });
    }

    /// ``UDSSVC_ARCH_0017`` — a refused write becomes 0x14 whatever the handler said.
    #[test]
    fn a_refused_write_settles_response_too_long() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        let _ = out.write_all(&[0x62, 0xF1, 0x90, 1, 2]);
        let no = Cell::new(false);
        let r = settle(ai(TaType::Physical), sid(0x22), &no, Ok(None), &mut out);
        assert_eq!(r, Responded::Yes { session: None });
        assert_eq!(out.written_bytes(), &[0x7F, 0x22, 0x14]);
    }
}
