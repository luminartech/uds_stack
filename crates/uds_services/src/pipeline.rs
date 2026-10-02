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
use crate::{
    DataIdentifier, DiagnosticSessionControl, ReadDataByIdentifier, TesterPresent,
};
use crate::{Responded, ResponseSink};
use automotive_wire_codec::Sink;
use core::cell::Cell;
use uds_protocol::{
    Decode, DiagnosticSessionType, Encode, NegativeResponse, NegativeResponseCode, Request,
    UdsServiceType,
};
use uds_protocol::{
    DiagnosticSessionControlRequest, ReadDataByIdentifierRequest, TesterPresentRequest,
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

/// ISO 14229-1:2020 clause 11.2 — `ReadDataByIdentifier`'s own stage
/// (``UDSSVC_ARCH_0008``).
///
/// Identifiers the application does not define are skipped; if none is defined the
/// request is `requestOutOfRange` (0x31); more than `MAX_DIDS_PER_REQUEST` is
/// `incorrectMessageLengthOrInvalidFormat` (0x13). The pipeline writes each identifier;
/// the handler writes its record.
#[doc(hidden)]
pub async fn read_data_by_identifier<A: ReadDataByIdentifier>(
    services: &mut A,
    request: &ReadDataByIdentifierRequest<'_>,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    if request.dids().count() > A::MAX_DIDS_PER_REQUEST {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    let _ = out.write_all(&[0x62]);
    let mut any = false;
    for raw in request.dids() {
        let Some(did) = <A::Did as DataIdentifier>::from_u16(raw) else {
            continue;
        };
        any = true;
        let _ = out.write_all(&raw.to_be_bytes());
        services.read(did, out).await?;
    }
    if any {
        Ok(None)
    } else {
        Err(NegativeResponseCode::RequestOutOfRange)
    }
}

/// ISO 14229-1:2020 clause 10.2 — `DiagnosticSessionControl`'s own stage
/// (``UDSSVC_ARCH_0035``): an unsupported session is `subFunctionNotSupported` (0x12);
/// otherwise the positive response carries the session and the application's timing,
/// `P2*` in 10 ms units (Table 29), and the session is selected.
#[doc(hidden)]
pub fn diagnostic_session_control<A: DiagnosticSessionControl>(
    services: &mut A,
    request: &DiagnosticSessionControlRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let session = request.session_type;
    if !services.supports(session) {
        return Err(NegativeResponseCode::SubFunctionNotSupported);
    }
    let timing = services.timing(session);
    let p2 = u16::try_from(timing.p2_server_max).unwrap_or(u16::MAX);
    let p2_star = u16::try_from(timing.p2_star_server_max / 10).unwrap_or(u16::MAX);
    let _ = out.write_all(&[0x50]);
    let _ = uds_protocol::DiagnosticSessionControlResponse::new(session, p2, p2_star)
        .encode(out);
    Ok(Some(session))
}

/// ISO 14229-1:2020 clause 10.6 — `TesterPresent`'s own stage (``UDSSVC_ARCH_0004``).
///
/// A sub-function other than `zeroSubFunction` (`0x01..=0x7F` with the suppress bit
/// stripped, ISO/SAE reserved) is `subFunctionNotSupported` (0x12), and the application
/// is not told. Otherwise the application is told and the positive response is `7E 00`;
/// whether it is sent is the suppress bit's, read by `settle`.
#[doc(hidden)]
pub fn tester_present<A: TesterPresent>(
    services: &mut A,
    request: &TesterPresentRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    if request.sub_function() != 0x00 {
        return Err(NegativeResponseCode::SubFunctionNotSupported);
    }
    services.on_tester_present();
    let _ = out.write_all(&[0x7E]);
    let _ = uds_protocol::TesterPresentResponse::new().encode(out);
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
mod tests {
    use super::{Settling, Stage, allowed_in_session, begin, settle, suppresses};
    use crate::state::{ProtocolState, State};
    use crate::{Responded, ResponseSink};
    use automotive_wire_codec::Sink;
    use core::cell::Cell;
    use uds_protocol::NegativeResponseCode as N;
    use uds_protocol::{DiagnosticSessionType as S, UdsServiceType as U};
    use uds_session::{Address, Ai, Mtype, TaType};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Did {
        Speed,
        Vin,
    }
    impl crate::DataIdentifier for Did {
        const MAX_RECORD_LEN: usize = 17;
        fn as_u16(self) -> u16 {
            match self {
                Self::Speed => 0xF40D,
                Self::Vin => 0xF190,
            }
        }
        fn from_u16(v: u16) -> Option<Self> {
            match v {
                0xF40D => Some(Self::Speed),
                0xF190 => Some(Self::Vin),
                _ => None,
            }
        }
        fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), crate::RecordError> {
            buf.split_at_checked(match self {
                Self::Speed => 1,
                Self::Vin => 17,
            })
            .ok_or(crate::RecordError::Short)
        }
    }
    struct Ecu;
    impl crate::ReadDataByIdentifier for Ecu {
        type Did = Did;
        const MAY_RESPOND_PENDING: bool = false;
        const MAX_DIDS_PER_REQUEST: usize = 2;
        // A plain `fn` returning a ready future: an `async fn` with no `.await` is
        // `clippy::unused_async_trait_impl`, which pedantic denies.
        fn read(
            &mut self,
            did: Did,
            out: &mut ResponseSink<'_>,
        ) -> impl core::future::Future<Output = Result<(), N>> {
            let _ = match did {
                Did::Speed => out.write_all(&[0x40]),
                Did::Vin => out.write_all(&[0x11; 17]),
            };
            core::future::ready(Ok(()))
        }
    }
    impl crate::DiagnosticSessionControl for Ecu {
        const MAX_RESPONSE_LEN: usize = 0;
        fn supports(&self, s: S) -> bool {
            matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
        }
        fn timing(&self, _s: S) -> crate::SessionTiming {
            crate::SessionTiming {
                p2_server_max: 50,
                p2_star_server_max: 5_000,
            }
        }
        fn on_transition(&mut self, _t: crate::SessionTransition, _r: bool) {}
    }
    impl crate::TesterPresent for Ecu {
        fn on_tester_present(&mut self) {}
    }

    fn block_on<F: core::future::Future>(f: F) -> F::Output {
        // The handlers never await anything, so one poll completes them.
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        let mut f = core::pin::pin!(f);
        match f.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(v) => v,
            core::task::Poll::Pending => panic!("a milestone-1 handler never pends"),
        }
    }

    /// Clause 11.2 — a supported identifier's record follows its identifier.
    #[test]
    fn rdbi_writes_identifier_then_record() {
        let mut buf = [0_u8; 32];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::ReadDataByIdentifierRequest::new(&[0xF40D]);
        let r = block_on(super::read_data_by_identifier(&mut Ecu, &req, &mut out));
        assert_eq!(r, Ok(None));
        assert_eq!(out.written_bytes(), &[0x62, 0xF4, 0x0D, 0x40]);
    }

    /// Clause 11.2 — unsupported identifiers are skipped; none supported is 0x31; too
    /// many is 0x13. (Review focus 4.)
    #[test]
    fn rdbi_partial_none_and_too_many() {
        let mut buf = [0_u8; 32];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::ReadDataByIdentifierRequest::new(&[0x0001, 0xF40D]);
        assert_eq!(
            block_on(super::read_data_by_identifier(&mut Ecu, &req, &mut out)),
            Ok(None)
        );
        assert_eq!(out.written_bytes(), &[0x62, 0xF4, 0x0D, 0x40]);
        out.rewind();
        let req = uds_protocol::ReadDataByIdentifierRequest::new(&[0x0001]);
        assert_eq!(
            block_on(super::read_data_by_identifier(&mut Ecu, &req, &mut out)),
            Err(N::RequestOutOfRange)
        );
        out.rewind();
        let req = uds_protocol::ReadDataByIdentifierRequest::new(&[0xF40D, 0xF190, 0xF40D]);
        assert_eq!(
            block_on(super::read_data_by_identifier(&mut Ecu, &req, &mut out)),
            Err(N::IncorrectMessageLengthOrInvalidFormat)
        );
    }

    /// Clause 10.2 — the positive response carries the session and its timing, P2* in
    /// 10 ms units, and selects the session.
    #[test]
    fn dsc_writes_timing_and_selects_the_session() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::DiagnosticSessionControlRequest::new(
            false,
            S::ExtendedDiagnosticSession,
        );
        let r = super::diagnostic_session_control(&mut Ecu, &req, &mut out);
        assert_eq!(r, Ok(Some(S::ExtendedDiagnosticSession)));
        assert_eq!(out.written_bytes(), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    }

    /// Clause 10.2 Figure 11 — an unsupported session is 0x12.
    #[test]
    fn dsc_refuses_an_unsupported_session() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::DiagnosticSessionControlRequest::new(
            false,
            S::ProgrammingSession,
        );
        assert_eq!(
            super::diagnostic_session_control(&mut Ecu, &req, &mut out),
            Err(N::SubFunctionNotSupported)
        );
    }

    /// Clause 10.6 — `TesterPresent` answers `7E 00` and tells the application.
    #[test]
    fn tester_present_answers_and_notifies() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::TesterPresentRequest::new(true);
        assert_eq!(super::tester_present(&mut Ecu, &req, &mut out), Ok(None));
        assert_eq!(out.written_bytes(), &[0x7E, 0x00]);
    }

    fn decode_tester_present(byte: u8) -> uds_protocol::TesterPresentRequest {
        let Ok((req, [])) =
            <uds_protocol::TesterPresentRequest as uds_protocol::Decode>::decode(&[byte])
        else {
            panic!("3E {byte:02X} decodes");
        };
        req
    }

    /// Clause 10.6 — a reserved sub-function is 0x12 with nothing written; `3E 00` and
    /// `3E 80` (suppress bit set, sub-function zero) both answer `7E 00`.
    #[test]
    fn tester_present_refuses_a_reserved_sub_function() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = decode_tester_present(0x05);
        assert_eq!(
            super::tester_present(&mut Ecu, &req, &mut out),
            Err(N::SubFunctionNotSupported)
        );
        assert_eq!(out.written_bytes(), &[0_u8; 0]);
        for byte in [0x00, 0x80] {
            out.rewind();
            let req = decode_tester_present(byte);
            assert_eq!(super::tester_present(&mut Ecu, &req, &mut out), Ok(None));
            assert_eq!(out.written_bytes(), &[0x7E, 0x00], "3E {byte:02X}");
        }
    }

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
