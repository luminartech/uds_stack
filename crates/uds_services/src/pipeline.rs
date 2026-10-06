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
use crate::{Access, Responded, ResponseSink, Sessions, Unsettled};
use crate::{
    ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataIdentifier,
    Delay, DiagnosticSessionControl, EcuReset, KeyVerdict, ReadDataByIdentifier,
    ReadDtcInformation, RecordError, RoutineControl, RoutineIdentifier, SecurityAccess,
    SecurityLevel, SecurityPolicy, TesterPresent, WriteDataByIdentifier,
};
use automotive_wire_codec::Sink;
use uds_protocol::{
    ClearDiagnosticInfoRequest, CommunicationControlRequest, CommunicationType,
    ControlDtcSettingRequest, DiagnosticSessionControlRequest, EcuResetRequest,
    ReadDataByIdentifierRequest, ReadDtcInfoRequest, RoutineControlRequest,
    RoutineControlSubFunction, SecurityAccessRequest, WriteDataByIdentifierRequest,
};
use uds_protocol::{
    Decode, DiagnosticSessionType, Encode, NegativeResponse, NegativeResponseCode, Request,
    UdsServiceType,
};
use uds_session::{Ai, TaType};

/// What entering a session did, for `DiagnosticSessionControl::on_transition`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entered {
    /// Which of Figure 7's transitions it was (``UDSSVC_ARCH_0038``).
    pub transition: SessionTransition,
    /// The session entered.
    pub session: DiagnosticSessionType,
    /// Whether it locked a security level that had been unlocked.
    pub security_relocked: bool,
}

/// Enter `to`, locking every security level (ISO 14229-1:2020 Annex I transition 6), and
/// say what that did. The one place the session field is written; the macro's hooks call
/// this and nothing else, so `State`'s accessors stay crate-private and no clause 10.2
/// logic is emitted into the application's crate.
#[doc(hidden)]
pub fn transition(state: &mut State, to: DiagnosticSessionType) -> Entered {
    let from = state.session();
    state.set_session(to);
    Entered {
        transition: SessionTransition::classify(from, to),
        session: to,
        security_relocked: state.lock(),
    }
}

/// The `P2` pair of the session `state` is in, as `services` states it. Read through here
/// for the reason [`transition`] writes through here: `State`'s accessors stay
/// crate-private.
#[doc(hidden)]
pub fn session_timing<A: DiagnosticSessionControl>(
    services: &A,
    state: &State,
) -> crate::SessionTiming {
    services.timing(state.session())
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
    /// No service identifier at all (``UDSSVC_ARCH_0005``): complete, with no response.
    Empty,
}

/// The common stages, in ISO 14229-1:2020 8.7.2 Figure 5's order (``UDSSVC_ARCH_0006``):
///
/// 1. No service identifier at all: [`Stage::Empty`] (``UDSSVC_ARCH_0005``'s declared
///    reading — the standard does not model the case).
/// 2. Service identifier supported? An unmodelled byte, or a service the assembly list
///    does not name, settles `serviceNotSupported` (0x11).
/// 3. Supported in the active session? Table 23's refusals settle
///    `serviceNotSupportedInActiveSession` (0x7F).
/// 4. For a service with a sub-function, other than 0x31, Figure 6's sub-function checks
///    (``UDSSVC_ARCH_0007``), in its order, from the one [`Access`] `access` returns: no
///    access settles `subFunctionNotSupported` (0x12) (row 2); an active session outside
///    [`Access::sessions`] settles `subFunctionNotSupportedInActiveSession` (0x7E) (row
///    4), so 0x7E is sent only for a sub-function supported somewhere, as Annex A
///    requires; and none of [`Access::levels`] unlocked in [`State`] settles
///    `securityAccessDenied` (0x33), Figure 6's optional sub-function security check.
///    All three read the byte after the service identifier, with
///    `suppressPosRspMsgIndicationBit` stripped, before any exact-length test, so a
///    trailing byte cannot turn 0x12, 0x7E or 0x33 into 0x13. A request with no such byte
///    fails Figure 6's minimum-length check, which the decode in 5 settles. Row 3,
///    authentication (0x34), is unconditionally true, as in Figure 5.
/// 5. Only then the service-specific check, where length and format live: a decode
///    failure settles the code `uds_protocol` assigns it, `requestOutOfRange` (0x31) for
///    a parameter outside its range and otherwise `incorrectMessageLengthOrInvalidFormat`
///    (0x13), as do bytes left over (``UDSSVC_ARCH_0005``).
///
/// Clause 8.7.5's pseudo-code agrees: its outer `SWITCH` on the service identifier falls
/// to `DEFAULT: responseCode = SNS` before any `message_length` test, so a malformed
/// request for a service this server lacks is 0x11, not 0x13; and its inner `SWITCH` on
/// the sub-function falls to `DEFAULT: responseCode = SFNS` before the length test of a
/// supported sub-function's arm. Figure 5's authentication check (0x34), between 2 and 3,
/// is unconditionally true here (architecture open question 4). Figure 5's optional SID
/// security check is not evaluated here: no service this crate stages requires a level
/// for its service identifier alone, and a service whose requirement turns on an
/// identifier in its data-parameters checks it in its own stage.
#[doc(hidden)]
#[must_use]
pub fn begin<'a, F, G>(
    state: &State,
    request: &'a [u8],
    supports: F,
    access: G,
) -> Stage<'a>
where
    F: Fn(UdsServiceType) -> bool,
    G: Fn(UdsServiceType, u8, DiagnosticSessionType) -> Option<Access>,
{
    let Some((&sid, parameters)) = request.split_first() else {
        return Stage::Empty;
    };
    let service = UdsServiceType::from_request_sid(sid);
    if matches!(service, UdsServiceType::UnsupportedDiagnosticService) || !supports(service)
    {
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
    if enters_sub_function_stage(service)
        && let Some(&byte) = parameters.first()
    {
        let Some(access) = access(service, byte & SUB_FUNCTION_VALUE, state.session())
        else {
            return Stage::Settle {
                sid,
                nrc: NegativeResponseCode::SubFunctionNotSupported,
            };
        };
        if !access.sessions().contains(state.session()) {
            return Stage::Settle {
                sid,
                nrc: NegativeResponseCode::SubFunctionNotSupportedInActiveSession,
            };
        }
        if !access.admits(state.unlocked()) {
            return Stage::Settle {
                sid,
                nrc: NegativeResponseCode::SecurityAccessDenied,
            };
        }
    }
    match Request::decode(request) {
        // A service `uds_protocol` names but does not model has no stage either: 0x11,
        // as for an unmodelled byte, and never 0x13 for a request that may be well formed.
        Ok((Request::Other { .. }, _)) => Stage::Settle {
            sid,
            nrc: NegativeResponseCode::ServiceNotSupported,
        },
        Ok((decoded, [])) => Stage::Proceed {
            sid,
            request: decoded,
        },
        Ok(_) => Stage::Settle {
            sid,
            nrc: NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
        },
        Err(error) => Stage::Settle {
            sid,
            nrc: error
                .negative_response_code()
                .unwrap_or(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat),
        },
    }
}

/// The sub-function's value bits: bit 7 of the byte is `suppressPosRspMsgIndicationBit`
/// (ISO 14229-1:2020 clause 9.2.2), which says nothing about whether the sub-function
/// is supported.
const SUB_FUNCTION_VALUE: u8 = 0x7F;

/// Whether `service` passes through Figure 6's sub-function stage: it carries a
/// sub-function, and it is not `RoutineControl` (0x31), which Figure 5's decision node
/// excludes (``UDSSVC_ARCH_0007``).
const fn enters_sub_function_stage(service: UdsServiceType) -> bool {
    matches!(service.has_sub_function(), Some(true))
        && !matches!(service, UdsServiceType::RoutineControl)
}

/// ISO 14229-1:2020 8.7.3.1 Figure 6 — the [`Access`] of the sub-function `value`
/// (suppress bit stripped) names, where `T`, the service's sub-function type, accepts the
/// value and `access` has one for it. [`begin`]'s lookup for every service whose trait
/// answers with an [`Access`] (``UDSSVC_ARCH_0007``).
#[doc(hidden)]
pub fn sub_function_access<T: TryFrom<u8>>(
    value: u8,
    access: impl FnOnce(T) -> Option<Access>,
) -> Option<Access> {
    T::try_from(value).ok().and_then(access)
}

/// ISO 14229-1:2020 clause 10.2 — `DiagnosticSessionControl`'s lookups as [`begin`] asks
/// them: no [`Access`] for a session the application does not support, and otherwise one
/// available in `active` exactly where the session may be entered from it. No level is
/// required: entering a session locks every level.
#[doc(hidden)]
#[must_use]
pub fn session_access<A: DiagnosticSessionControl>(
    services: &A,
    value: u8,
    active: DiagnosticSessionType,
) -> Option<Access> {
    let session = DiagnosticSessionType::try_from(value)
        .ok()
        .filter(|&session| services.supports(session))?;
    let from = if services.supported_from(session, active) {
        Sessions::NONE.with(active)
    } else {
        Sessions::NONE
    };
    Some(Access::new(from))
}

/// The level a `SecurityAccess` sub-function `value` (suppress bit stripped) names: its
/// own for a `requestSeed`, its partner's for a `sendKey` (clause 10.4.2).
const fn security_level(value: u8) -> Option<SecurityLevel> {
    SecurityLevel::from_request_seed(if value.is_multiple_of(2) {
        value.wrapping_sub(1)
    } else {
        value
    })
}

/// ISO 14229-1:2020 clause 10.4 — `SecurityAccess`'s lookup as [`begin`] asks it: the
/// sessions in which the level the sub-function `value` (suppress bit stripped) names is
/// available, the `requestSeed` and `sendKey` alike, with no level required.
#[doc(hidden)]
#[must_use]
pub fn security_access_sessions<A: SecurityAccess>(
    services: &A,
    value: u8,
) -> Option<Access> {
    security_level(value)
        .and_then(|level| services.sessions(level))
        .map(Access::new)
}

/// ISO 14229-1:2020 clause 10.7 — `TesterPresent`'s lookup as [`begin`] asks it:
/// `zeroSubFunction`, the only one the service defines, is available in every session
/// with no level required (Table 23).
#[doc(hidden)]
#[must_use]
pub const fn zero_sub_function(value: u8) -> Option<Access> {
    if value == 0x00 {
        Some(Access::new(Sessions::ALL))
    } else {
        None
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settling {
    /// `SIDRQ` — the request's service identifier, echoed by a negative response.
    pub sid: u8,
    /// `suppressPosRspMsgIndicationBit`, where the service has a sub-function byte.
    pub suppress_bit: bool,
}

/// The last two stages: "response fits" (``UDSSVC_ARCH_0017`` → 0x14) and the
/// suppression gate (``UDSSVC_ARCH_0009``). Writes a negative response itself; the
/// handler's bytes, if any, are discarded by rewinding.
///
/// Called by the driver once the handler future has completed and before the response is
/// submitted, never by the emitted `dispatch`: `pending_sent`, rule 3's input, is the
/// driver's to know (see [`Unsettled`]). It is whether a 0x78 for this request was
/// accepted for transmission while the handler ran, and it lifts both suppressions.
#[doc(hidden)]
pub fn settle(
    ai: Ai,
    unsettled: Unsettled,
    pending_sent: bool,
    out: &mut ResponseSink<'_>,
) -> Responded {
    // ``UDSSVC_ARCH_0005`` — no service identifier: complete, and no rule reaches it.
    let Some((settling, outcome)) = unsettled.parts() else {
        return Responded::Suppressed { session: None };
    };
    let Settling { sid, suppress_bit } = settling;
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
            // be refused: `uds_server!` holds the buffer to `ResponseSink::MIN_BOUND`, and
            // `ResponseSink::new` holds the bound to it.
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

/// ISO 14229-1:2020 clause 11.7, Figure 26 — the check `WriteDataByIdentifier` makes
/// ahead of the record's length: an identifier the application does not define, or one
/// [`WriteDataByIdentifier::access`] does not admit in the active session, is
/// `requestOutOfRange` (0x31). Asked by [`write_data_by_identifier`], and by `dispatch`
/// for a request too long to have been received whole, which is 0x13 only once this
/// passes.
///
/// # Errors
///
/// `requestOutOfRange` (0x31), as above.
#[doc(hidden)]
pub fn writable_identifier<A: WriteDataByIdentifier>(
    services: &A,
    state: &State,
    request: &WriteDataByIdentifierRequest<'_>,
) -> Result<(A::Did, Access), NegativeResponseCode> {
    <A::Did as DataIdentifier>::from_u16(request.identifier)
        .and_then(|did| services.access(did).map(|access| (did, access)))
        .filter(|(_, access)| access.sessions().contains(state.session()))
        .ok_or(NegativeResponseCode::RequestOutOfRange)
}

/// ISO 14229-1:2020 clause 11.7 — `WriteDataByIdentifier`'s own stage, in Figure 26's
/// order. The decode settled a request without a data record (0x13); then:
///
/// * [`writable_identifier`]'s check (0x31);
/// * a record [`DataIdentifier::split_record`] finds short, or followed by further bytes,
///   is `incorrectMessageLengthOrInvalidFormat` (0x13);
/// * an identifier none of whose [`Access::levels`] `state` holds unlocked is
///   `securityAccessDenied` (0x33);
/// * a record `split_record` finds malformed is `requestOutOfRange` (0x31).
///
/// The handler's verdict decides the rest, and the positive response is `6E` and the
/// echoed identifier (Table 279).
#[doc(hidden)]
pub async fn write_data_by_identifier<A: WriteDataByIdentifier>(
    services: &mut A,
    state: &State,
    request: &WriteDataByIdentifierRequest<'_>,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let (did, access) = writable_identifier(services, state, request)?;
    let split = did.split_record(request.data());
    if matches!(split, Err(RecordError::Short) | Ok((_, [_, ..]))) {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    if !access.admits(state.unlocked()) {
        return Err(NegativeResponseCode::SecurityAccessDenied);
    }
    let Ok((record, _)) = split else {
        return Err(NegativeResponseCode::RequestOutOfRange);
    };
    services.write(did, record).await?;
    let _ = out.write_all(&[0x6E]);
    let _ = out.write_all(&request.identifier.to_be_bytes());
    Ok(None)
}

/// The three `routineControlType`s Table 426 defines, each its own [`RoutineControl`]
/// method.
#[derive(Debug, Clone, Copy)]
enum Routine {
    Start,
    Stop,
    Results,
}

impl Routine {
    const fn of(control: RoutineControlSubFunction) -> Option<Self> {
        match control {
            RoutineControlSubFunction::StartRoutine => Some(Self::Start),
            RoutineControlSubFunction::StopRoutine => Some(Self::Stop),
            RoutineControlSubFunction::RequestRoutineResults => Some(Self::Results),
            _ => None,
        }
    }
}

/// ISO 14229-1:2020 clause 14.2, Figure 30 — the checks `RoutineControl` makes ahead of
/// the option record's length, in order: an identifier the application does not define,
/// or one [`RoutineControl::access`] does not admit in the active session, is
/// `requestOutOfRange` (0x31); a routine none of whose [`Access::levels`] `state` holds
/// unlocked is `securityAccessDenied` (0x33); and a `routineControlType` Table 426
/// reserves, or one [`RoutineControl::supports`] refuses for this routine, is
/// `subFunctionNotSupported` (0x12). Asked by [`routine_control`], and by `dispatch` for a
/// request too long to have been received whole, which is 0x13 only once these pass.
///
/// # Errors
///
/// 0x31, 0x33 or 0x12, as above.
#[doc(hidden)]
pub fn admitted_routine<A: RoutineControl>(
    services: &A,
    state: &State,
    request: &RoutineControlRequest<'_>,
) -> Result<A::Rid, NegativeResponseCode> {
    admitted(services, state, request).map(|(routine, _)| routine)
}

fn admitted<A: RoutineControl>(
    services: &A,
    state: &State,
    request: &RoutineControlRequest<'_>,
) -> Result<(A::Rid, Routine), NegativeResponseCode> {
    let Some((routine, access)) =
        <A::Rid as RoutineIdentifier>::from_u16(request.routine_id)
            .and_then(|routine| services.access(routine).map(|access| (routine, access)))
            .filter(|(_, access)| access.sessions().contains(state.session()))
    else {
        return Err(NegativeResponseCode::RequestOutOfRange);
    };
    if !access.admits(state.unlocked()) {
        return Err(NegativeResponseCode::SecurityAccessDenied);
    }
    Routine::of(request.sub_function)
        .filter(|_| services.supports(routine, request.sub_function))
        .map(|method| (routine, method))
        .ok_or(NegativeResponseCode::SubFunctionNotSupported)
}

/// ISO 14229-1:2020 clause 14.2 — `RoutineControl`'s own stage, in Figure 30's order.
/// The decode settled a request shorter than its routine identifier (0x13); then:
///
/// * [`admitted_routine`]'s checks (0x31, 0x33, 0x12);
/// * an option record longer than [`RoutineControl::MAX_OPTION_LEN`] is
///   `incorrectMessageLengthOrInvalidFormat` (0x13).
///
/// The pipeline then writes `71`, the echoed `routineControlType` and the identifier, and
/// the sub-function's own method writes the rest and decides Figure 30's remaining checks.
#[doc(hidden)]
pub async fn routine_control<A: RoutineControl>(
    services: &mut A,
    state: &State,
    request: &RoutineControlRequest<'_>,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let (routine, method) = admitted(services, state, request)?;
    let record = request.option_record;
    if record.len() > A::MAX_OPTION_LEN {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    let _ = out.write_all(&[0x71, u8::from(request.sub_function)]);
    let _ = out.write_all(&request.routine_id.to_be_bytes());
    match method {
        Routine::Start => services.start(routine, record, out).await,
        Routine::Stop => services.stop(routine, record, out).await,
        Routine::Results => services.results(routine, record, out).await,
    }?;
    Ok(None)
}

/// ISO 14229-1:2020 clause 12.3 — `ReadDTCInformation`'s own stage. Whether the report
/// type is supported (0x12), in the active session (0x7E) and unlocked (0x33) was settled
/// by [`begin`], and its parameters' exact length by the decode (0x13). The pipeline
/// writes `59` and the echoed report type; the handler writes the rest of the layout
/// (clause 12.3.3) and decides 0x31.
#[doc(hidden)]
pub async fn read_dtc_information<A: ReadDtcInformation>(
    services: &mut A,
    request: &ReadDtcInfoRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let _ = out.write_all(&[0x59, request.dtc_subfunction.value()]);
    services
        .read_dtc_information(request.dtc_subfunction, out)
        .await?;
    Ok(None)
}

/// ISO 14229-1:2020 clause 12.2 — `ClearDiagnosticInformation`'s own stage. A request
/// that is not `groupOfDTC` and an optional `MemorySelection` was settled 0x13 by its
/// decode (Figure 28); the handler's verdict decides the rest, and the positive response
/// is `54` alone (Table 298).
#[doc(hidden)]
pub async fn clear_diagnostic_information<A: ClearDiagnosticInformation>(
    services: &mut A,
    request: &ClearDiagnosticInfoRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    services
        .clear(request.group_of_dtc, request.memory_selection)
        .await?;
    let _ = out.write_all(&[0x54]);
    Ok(None)
}

/// ISO 14229-1:2020 clause 10.2 — `DiagnosticSessionControl`'s own stage
/// (``UDSSVC_ARCH_0035``): the positive response carries the session and the
/// application's timing, and the session is selected. Whether the session is supported
/// at all (0x12) and from the active one (0x7E) was settled by [`begin`] before the
/// request was decoded, so it is not asked again here.
///
/// [`SessionTiming`](crate::SessionTiming) is held in Table 29's wire form, so it is sent
/// as stated.
#[doc(hidden)]
pub fn diagnostic_session_control<A: DiagnosticSessionControl>(
    services: &mut A,
    request: &DiagnosticSessionControlRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let session = request.session_type;
    let timing = services.timing(session);
    let _ = out.write_all(&[0x50]);
    let _ = uds_protocol::DiagnosticSessionControlResponse::new(
        session,
        timing.p2_server_max_ms,
        timing.p2_star_server_max_10ms,
    )
    .encode(out);
    Ok(Some(session))
}

/// ISO 14229-1:2020 clause 10.3 — `EcuReset`'s own stage. Whether the reset is supported
/// (0x12) and in the active session (0x7E) was settled by [`begin`], and the exact length
/// (0x13) by its decode, so only the handler's verdict remains: the positive response is
/// `51`, the echoed `resetType` (Table 35), then whatever `powerDownTime` the handler
/// wrote. The reset itself is the application's, after that response.
#[doc(hidden)]
pub async fn ecu_reset<A: EcuReset>(
    services: &mut A,
    request: &EcuResetRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let kind = request.reset_type;
    let _ = out.write_all(&[0x51, u8::from(kind)]);
    services.reset(kind, out).await?;
    Ok(None)
}

/// ISO 14229-1:2020 clause 10.4 and Annex I — `SecurityAccess`'s own stage
/// (``UDSSVC_ARCH_0037``). Whether the level is supported (0x12) and in the active session
/// (0x7E) was settled by [`begin`]; this runs Figure I.1 against `state`.
///
/// A `requestSeed`, in Table I.2's order:
///
/// * carrying a `securityAccessDataRecord` longer than
///   [`SecurityAccess::MAX_RECORD_LEN`] is `incorrectMessageLengthOrInvalidFormat` (0x13);
/// * where [`SecurityAccess::preconditions_met`] refuses is `conditionsNotCorrect` (0x22)
///   (transition 4);
/// * while the level's delay runs is `requiredTimeDelayNotExpired` (0x37) (transition 4);
/// * for the unlocked level is answered with a zero seed of
///   [`SecurityAccess::MAX_SEED_LEN`] bytes, discarding any seed awaiting a key
///   (transitions 7 and 10);
/// * otherwise is answered with the application's seed, and its level becomes the one
///   whose key is awaited (transitions 2, 5 and 8). A delay supported by the policy and
///   no longer running has expired, so an attempt count at the limit is reset first.
///
/// A `sendKey` discards the awaited seed whatever its outcome (transitions 9 and 10), and
/// is:
///
/// * `requestSequenceError` (0x24) where no seed awaits a key, or it is another level's;
/// * `incorrectMessageLengthOrInvalidFormat` (0x13) for an empty key or one longer than
///   [`SecurityAccess::MAX_KEY_LEN`];
/// * on a valid key, positive: the level is unlocked, any other locked, and its attempt
///   count reset (transitions 3 and 10);
/// * on an invalid key, `invalidKey` (0x35), or `exceedNumberOfAttempts` (0x36) once
///   `(Att_Cnt + 1) >= Att_Cnt_Limit`, which clamps the count at the limit and starts the
///   delay where the policy keeps one (transitions 9 and 10).
#[doc(hidden)]
pub async fn security_access<A: SecurityAccess>(
    services: &mut A,
    state: &mut State,
    request: &SecurityAccessRequest<'_>,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let value = u8::from(request.access_type);
    let Some(level) = security_level(value) else {
        return Err(NegativeResponseCode::SubFunctionNotSupported);
    };
    if value == level.request_seed() {
        request_seed(services, state, level, request.request_data, out).await?;
    } else {
        send_key(services, state, level, request.request_data, out).await?;
    }
    Ok(None)
}

async fn request_seed<A: SecurityAccess>(
    services: &mut A,
    state: &mut State,
    level: SecurityLevel,
    record: &[u8],
    out: &mut ResponseSink<'_>,
) -> Result<(), NegativeResponseCode> {
    if record.len() > A::MAX_RECORD_LEN {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    if !services.preconditions_met(level) {
        return Err(NegativeResponseCode::ConditionsNotCorrect);
    }
    if delay_owed(services, level) {
        return Err(NegativeResponseCode::RequiredTimeDelayNotExpired);
    }
    let _ = out.write_all(&[0x67, level.request_seed()]);
    if state.unlocked() == Some(level) {
        state.take_seed();
        for _ in 0..A::MAX_SEED_LEN {
            let _ = out.write_all(&[0x00]);
        }
        return Ok(());
    }
    services.seed(level, record, out).await?;
    state.seed_sent(level);
    Ok(())
}

/// Annex I — whether `level` owes a delay now: one is running, or none is and the
/// stored attempt count is at the limit, which starts it (transition 1). An expired delay
/// resets the count (Table I.2). False for a level whose policy keeps no delay.
fn delay_owed<A: SecurityAccess>(services: &mut A, level: SecurityLevel) -> bool {
    let SecurityPolicy::Counted {
        attempt_limit,
        delay_ms: Some(_),
        ..
    } = services.policy(level)
    else {
        return false;
    };
    match services.delay(level) {
        Delay::Running => true,
        Delay::Expired => {
            services.store_attempts(level, 0);
            false
        }
        Delay::Idle if services.load_attempts(level) >= attempt_limit => {
            services.start_delay(level);
            true
        }
        Delay::Idle => false,
    }
}

/// ISO 14229-1:2020 Annex I, Table I.2 transition 1 — at start-up, start the delay of
/// every level `services` supports whose stored attempt count is at its limit, so a
/// lockout outlives a restart that a RAM delay timer does not.
#[doc(hidden)]
pub fn security_start_up<A: SecurityAccess>(services: &mut A) {
    for value in (0x01..0x7F).step_by(2) {
        if let Some(level) = SecurityLevel::from_request_seed(value)
            && services.sessions(level).is_some()
        {
            let _ = delay_owed(services, level);
        }
    }
}

async fn send_key<A: SecurityAccess>(
    services: &mut A,
    state: &mut State,
    level: SecurityLevel,
    key: &[u8],
    out: &mut ResponseSink<'_>,
) -> Result<(), NegativeResponseCode> {
    if state.take_seed() != Some(level) {
        return Err(NegativeResponseCode::RequestSequenceError);
    }
    if key.is_empty() || key.len() > A::MAX_KEY_LEN {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    let counted = matches!(services.policy(level), SecurityPolicy::Counted { .. });
    match services.verify_key(level, key).await? {
        KeyVerdict::Valid => {
            if counted {
                services.store_attempts(level, 0);
            }
            state.unlock(level);
            let _ = out.write_all(&[0x67, level.send_key()]);
            Ok(())
        }
        KeyVerdict::Invalid => Err(failed_attempt(services, level)),
    }
}

fn failed_attempt<A: SecurityAccess>(
    services: &mut A,
    level: SecurityLevel,
) -> NegativeResponseCode {
    let SecurityPolicy::Counted {
        attempt_limit,
        delay_ms,
        ..
    } = services.policy(level)
    else {
        return NegativeResponseCode::InvalidKey;
    };
    let count = services.load_attempts(level);
    if count.saturating_add(1) < attempt_limit {
        services.store_attempts(level, count.saturating_add(1));
        return NegativeResponseCode::InvalidKey;
    }
    services.store_attempts(level, attempt_limit);
    if delay_ms.is_some() {
        services.start_delay(level);
    }
    NegativeResponseCode::ExceedNumberOfAttempts
}

/// ISO 14229-1:2020 Annex I, Table I.2 transitions 9 and 10 — `unsettled`, having
/// discarded the seed awaiting a key where it refuses a `SecurityAccess` request, whichever
/// check refused it: the common stages, the length of a request too long to be received
/// whole, or the stage itself. Any other request leaves the seed.
#[doc(hidden)]
#[must_use]
pub fn discarding_seed(state: &mut State, unsettled: Unsettled) -> Unsettled {
    if let Some((settling, Err(_))) = unsettled.parts()
        && matches!(
            UdsServiceType::from_request_sid(settling.sid),
            UdsServiceType::SecurityAccess
        )
    {
        state.take_seed();
    }
    unsettled
}

/// ISO 14229-1:2020 clause 10.5 — `CommunicationControl`'s own stage. Whether the
/// `controlType` is supported (0x12), in the active session (0x7E) and unlocked (0x33)
/// was settled by [`begin`], and the exact length, with `nodeIdentificationNumber`
/// present exactly for the enhanced-address variants, by its decode (0x13), as was a
/// `communicationType` with its reserved bits 3-2 set (0x31). One whose bits 1-0 are the
/// value Annex B Table B.1 also reserves is `requestOutOfRange` (0x31) here. The
/// handler's verdict decides the rest, and the positive response is `68` and the echoed
/// `controlType` (Table 56).
#[doc(hidden)]
pub async fn communication_control<A: CommunicationControl>(
    services: &mut A,
    request: &CommunicationControlRequest,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    let control_type = request.control_type();
    if matches!(
        request.communication_type(),
        CommunicationType::IsoSaeReserved
    ) {
        return Err(NegativeResponseCode::RequestOutOfRange);
    }
    services
        .control(
            control_type,
            request.communication_type(),
            request.subnet(),
            request.node_id(),
        )
        .await?;
    let _ = out.write_all(&[0x68, u8::from(control_type)]);
    Ok(None)
}

/// ISO 14229-1:2020 clause 10.8 — `ControlDTCSetting`'s own stage. Whether the setting
/// is supported (0x12), in the active session (0x7E) and unlocked (0x33) was settled by
/// [`begin`]; a `DTCSettingControlOptionRecord` longer than
/// [`ControlDtcSetting::MAX_OPTION_RECORD_LEN`] is `incorrectMessageLengthOrInvalidFormat`
/// (0x13). Otherwise the handler's verdict decides, and the positive response is `C5` and
/// the echoed `DTCSettingType` (Table 130).
#[doc(hidden)]
pub async fn control_dtc_setting<A: ControlDtcSetting>(
    services: &mut A,
    request: &ControlDtcSettingRequest<'_>,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    if request.option_record.len() > A::MAX_OPTION_RECORD_LEN {
        return Err(NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat);
    }
    services
        .control_dtc_setting(request.setting, request.option_record)
        .await?;
    let _ = out.write_all(&[0xC5, u8::from(request.setting)]);
    Ok(None)
}

/// ISO 14229-1:2020 clause 10.7 — `TesterPresent`'s own stage (``UDSSVC_ARCH_0004``).
///
/// A sub-function other than `zeroSubFunction` (`0x01..=0x7F` with the suppress bit
/// stripped, ISO/SAE reserved) is `subFunctionNotSupported` (0x12), settled by [`begin`]
/// through [`zero_sub_function`] before the request is decoded (``UDSSVC_ARCH_0007`` row
/// 2), so only `zeroSubFunction` reaches this stage: the application is told and the
/// positive response is `7E 00`. Whether it is sent is the suppress bit's, read by
/// `settle`.
#[doc(hidden)]
pub fn tester_present<A: TesterPresent>(
    services: &mut A,
    out: &mut ResponseSink<'_>,
) -> Result<Option<DiagnosticSessionType>, NegativeResponseCode> {
    services.on_tester_present();
    let _ = out.write_all(&[0x7E]);
    let _ = uds_protocol::TesterPresentResponse::new().encode(out);
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
mod tests {
    use super::{
        Settling, Stage, allowed_in_session, begin, session_access, settle, suppresses,
        zero_sub_function,
    };
    use crate::state::{ProtocolState, State};
    use crate::{Access, Sessions};
    use crate::{Responded, ResponseSink, Unsettled};
    use automotive_wire_codec::Sink;
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
            matches!(
                s,
                S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
            )
        }
        /// Programming is entered only from Extended; every other session from any.
        fn supported_from(&self, s: S, active: S) -> bool {
            !matches!(s, S::ProgrammingSession)
                || matches!(active, S::ExtendedDiagnosticSession)
        }
        fn timing(&self, _s: S) -> crate::SessionTiming {
            crate::SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max_10ms: 500,
            }
        }
        fn on_transition(&mut self, _t: crate::SessionTransition, _e: S, _r: bool) {}
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
    /// many is 0x13.
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

    /// An application whose timing is the widest Table 29's wire form carries.
    struct Odd;
    impl crate::DiagnosticSessionControl for Odd {
        const MAX_RESPONSE_LEN: usize = 0;
        fn supports(&self, _s: S) -> bool {
            true
        }
        fn supported_from(&self, _s: S, _active: S) -> bool {
            true
        }
        fn timing(&self, _s: S) -> crate::SessionTiming {
            crate::SessionTiming {
                p2_server_max_ms: u16::MAX,
                p2_star_server_max_10ms: u16::MAX,
            }
        }
        fn on_transition(&mut self, _t: crate::SessionTransition, _e: S, _r: bool) {}
    }

    /// Table 29 — the pair is sent as the application stated it, to the widest values.
    #[test]
    fn dsc_sends_the_timing_as_stated() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let req = uds_protocol::DiagnosticSessionControlRequest::new(
            false,
            S::ExtendedDiagnosticSession,
        );
        let r = super::diagnostic_session_control(&mut Odd, &req, &mut out);
        assert_eq!(r, Ok(Some(S::ExtendedDiagnosticSession)));
        assert_eq!(out.written_bytes(), &[0x50, 0x03, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    /// Clause 10.7 — `TesterPresent` answers `7E 00` and tells the application.
    #[test]
    fn tester_present_answers_and_notifies() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        assert_eq!(super::tester_present(&mut Ecu, &mut out), Ok(None));
        assert_eq!(out.written_bytes(), &[0x7E, 0x00]);
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

    /// The lookup `uds_server!` emits for an assembly listing `Ecu`'s services: each
    /// listed service with a stage answers for its own sub-function, and
    /// `TesterPresent`'s zero sub-function is available in every session (clause 10.7;
    /// Table 23). No sub-function of the test assembly requires a security level.
    fn ecu_access(service: U, value: u8, active: S) -> Option<Access> {
        match service {
            U::DiagnosticSessionControl => session_access(&Ecu, value, active),
            U::TesterPresent => zero_sub_function(value),
            _ => Some(Access::new(Sessions::ALL)),
        }
    }

    /// The same, refusing every session: what it supports is supported in none.
    fn ecu_access_nowhere(service: U, value: u8, active: S) -> Option<Access> {
        ecu_access(service, value, active).map(|_| Access::new(Sessions::NONE))
    }

    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` security check OK?" — a
    /// sub-function that either of two levels unlocks, as an OEM and a supplier level
    /// commonly share one, proceeds with either unlocked and is 0x33 with neither.
    #[test]
    #[allow(clippy::expect_used, reason = "the levels are odd requestSeed values")]
    fn a_sub_function_any_of_its_levels_unlocks_proceeds() {
        let level = |value| crate::SecurityLevel::from_request_seed(value).expect("odd");
        let (oem, supplier) = (level(0x01), level(0x11));
        let either = Access::new(Sessions::ALL)
            .unlocked_by(crate::Levels::NONE.with(oem).with(supplier));
        for (unlocked, nrc) in [
            (Some(oem), None),
            (Some(supplier), None),
            (Some(level(0x03)), Some(N::SecurityAccessDenied)),
            (None, Some(N::SecurityAccessDenied)),
        ] {
            let mut state = in_session(S::ExtendedDiagnosticSession);
            if let Some(level) = unlocked {
                state.unlock(level);
            }
            let st = begin(
                &state,
                &[0x11, 0x01],
                |s| matches!(s, U::EcuReset),
                |_, _, _| Some(either),
            );
            match nrc {
                None => assert!(matches!(st, Stage::Proceed { sid: 0x11, .. }), "{st:?}"),
                Some(nrc) => assert!(
                    matches!(st, Stage::Settle { sid: 0x11, nrc: got } if got == nrc),
                    "{unlocked:?}: {st:?}"
                ),
            }
        }
    }

    /// ``UDSSVC_ARCH_0005`` — an empty request is the pipeline's, and is `Empty`.
    #[test]
    fn an_empty_request_is_empty() {
        assert!(matches!(
            begin(&State::INITIAL, &[], supports_rdbi, ecu_access),
            Stage::Empty
        ));
    }

    /// ``UDSSVC_ARCH_0005`` — a decode failure settles 0x13 with the SID echoed.
    #[test]
    fn a_short_request_settles_0x13() {
        let st = begin(&State::INITIAL, &[0x22, 0xF1], supports_rdbi, ecu_access);
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
        let st = begin(&State::INITIAL, &[0x3E, 0x00], supports_rdbi, ecu_access);
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x3E,
                nrc: N::ServiceNotSupported
            }
        ));
        let st = begin(&State::INITIAL, &[0xBA, 0x00], supports_rdbi, ecu_access);
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
        let st = begin(
            &state,
            &[0x27, 0x01],
            |s| matches!(s, U::SecurityAccess),
            ecu_access,
        );
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x27,
                nrc: N::ServiceNotSupportedInActiveSession
            }
        ));
    }

    /// ``UDSSVC_ARCH_0006``, Figure 5 and clause 8.7.5 — support is checked before
    /// decoding: a malformed request for a service the list does not name is 0x11, not
    /// 0x13.
    #[test]
    fn a_malformed_request_for_an_unlisted_service_settles_0x11() {
        let st = begin(&State::INITIAL, &[0x3E], supports_rdbi, ecu_access);
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x3E,
                nrc: N::ServiceNotSupported
            }
        ));
    }

    /// ``UDSSVC_ARCH_0006`` check 3 precedes decoding: a malformed request for a Table 23
    /// service in the default session is 0x7F, not 0x13.
    #[test]
    fn a_malformed_request_refused_in_the_default_session_settles_0x7f() {
        let st = begin(
            &State::INITIAL,
            &[0x27],
            |s| matches!(s, U::SecurityAccess),
            ecu_access,
        );
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x27,
                nrc: N::ServiceNotSupportedInActiveSession
            }
        ));
    }

    /// ``UDSSVC_ARCH_0005`` — a listed service allowed in the session reaches the
    /// decode, and a malformed request for it is 0x13.
    #[test]
    fn a_malformed_request_for_a_listed_allowed_service_settles_0x13() {
        let st = begin(
            &State::INITIAL,
            &[0x3E],
            |s| matches!(s, U::TesterPresent),
            ecu_access,
        );
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x3E,
                nrc: N::IncorrectMessageLengthOrInvalidFormat
            }
        ));
    }

    /// A well-formed, supported request proceeds with its decoded form.
    #[test]
    fn a_good_request_proceeds() {
        let st = begin(
            &State::INITIAL,
            &[0x22, 0xF1, 0x90],
            supports_rdbi,
            ecu_access,
        );
        assert!(matches!(st, Stage::Proceed { sid: 0x22, .. }));
    }

    fn supports_ecu(s: U) -> bool {
        matches!(
            s,
            U::ReadDataByIdentifier | U::DiagnosticSessionControl | U::TesterPresent
        )
    }

    /// ``UDSSVC_ARCH_0007`` row 2 precedes the exact-length decode (Figure 6; clause
    /// 8.7.5's inner `SWITCH`): an unsupported sub-function with a trailing byte is 0x12,
    /// not 0x13, whether the service decides it from the byte alone (`TesterPresent`) or
    /// from the application (`DiagnosticSessionControl`), and with the suppress bit set.
    #[test]
    fn an_unsupported_sub_function_with_a_trailing_byte_settles_0x12() {
        for (request, sid) in [
            (&[0x3E, 0x05, 0x00][..], 0x3E),
            (&[0x3E, 0x85, 0x00][..], 0x3E),
            (&[0x10, 0x05, 0x00][..], 0x10),
            (&[0x10, 0x04, 0x00][..], 0x10),
        ] {
            let st = begin(&State::INITIAL, request, supports_ecu, ecu_access);
            assert!(
                matches!(
                    st,
                    Stage::Settle { sid: s, nrc: N::SubFunctionNotSupported } if s == sid
                ),
                "{request:02X?}: {st:?}"
            );
        }
    }

    /// A supported sub-function with a trailing byte passes row 2 and fails the
    /// service-specific length check: 0x13.
    #[test]
    fn a_supported_sub_function_with_a_trailing_byte_settles_0x13() {
        for request in [&[0x3E, 0x80, 0x00][..], &[0x10, 0x03, 0x00][..]] {
            let st = begin(&State::INITIAL, request, supports_ecu, ecu_access);
            assert!(
                matches!(
                    st,
                    Stage::Settle {
                        nrc: N::IncorrectMessageLengthOrInvalidFormat,
                        ..
                    }
                ),
                "{request:02X?}: {st:?}"
            );
        }
    }

    /// A listed, allowed, well-formed request with a supported sub-function proceeds.
    #[test]
    fn a_good_sub_function_request_proceeds() {
        for request in [&[0x3E, 0x00][..], &[0x3E, 0x80][..], &[0x10, 0x03][..]] {
            let st = begin(&State::INITIAL, request, supports_ecu, ecu_access);
            assert!(
                matches!(st, Stage::Proceed { .. }),
                "{request:02X?}: {st:?}"
            );
        }
    }

    /// A service without a sub-function never reaches row 2: a trailing byte on a
    /// malformed `ReadDataByIdentifier` is still 0x13, though the check would refuse
    /// every byte.
    #[test]
    fn a_malformed_request_without_a_sub_function_still_settles_0x13() {
        let st = begin(
            &State::INITIAL,
            &[0x22, 0xF1, 0x90, 0x00],
            supports_ecu,
            |_, _, _| None,
        );
        assert!(matches!(
            st,
            Stage::Settle {
                sid: 0x22,
                nrc: N::IncorrectMessageLengthOrInvalidFormat
            }
        ));
    }

    /// Figure 5's decision node excludes 0x31 from the sub-function stage
    /// (``UDSSVC_ARCH_0007``), so a check that refuses everything is never asked.
    #[test]
    fn routine_control_skips_the_sub_function_stage() {
        let st = begin(
            &State::INITIAL,
            &[0x31, 0x01, 0xFF, 0x00],
            |s| matches!(s, U::RoutineControl),
            |_, _, _| None,
        );
        assert!(!matches!(
            st,
            Stage::Settle {
                nrc: N::SubFunctionNotSupported,
                ..
            }
        ));
    }

    fn in_session(session: S) -> State {
        let mut state = State::INITIAL;
        state.set_session(session);
        state
    }

    /// ``UDSSVC_ARCH_0007`` row 2 — a session the application never supports is 0x12,
    /// and row 4 is not reached: a check refusing every session-from pair does not turn
    /// it into 0x7E.
    #[test]
    fn a_session_never_supported_settles_0x12() {
        let st = begin(
            &State::INITIAL,
            &[0x10, 0x04],
            supports_ecu,
            ecu_access_nowhere,
        );
        assert!(
            matches!(
                st,
                Stage::Settle {
                    sid: 0x10,
                    nrc: N::SubFunctionNotSupported
                }
            ),
            "{st:?}"
        );
    }

    /// ``UDSSVC_ARCH_0007`` row 4 — a session supported, but not from the active one,
    /// is 0x7E (Figure 6; Annex A), with or without the suppress bit, and before the
    /// exact-length test, so a trailing byte does not turn it into 0x13.
    #[test]
    fn a_session_not_supported_from_the_active_one_settles_0x7e() {
        for request in [
            &[0x10, 0x02][..],
            &[0x10, 0x82][..],
            &[0x10, 0x02, 0x00][..],
        ] {
            let st = begin(&State::INITIAL, request, supports_ecu, ecu_access);
            assert!(
                matches!(
                    st,
                    Stage::Settle {
                        sid: 0x10,
                        nrc: N::SubFunctionNotSupportedInActiveSession
                    }
                ),
                "{request:02X?}: {st:?}"
            );
        }
    }

    /// ``UDSSVC_ARCH_0007`` rows 2 and 4 both pass: the same `10 02` proceeds from the
    /// session the application allows it from, and the active session is the one read.
    #[test]
    fn a_session_supported_from_the_active_one_proceeds() {
        let st = begin(
            &in_session(S::ExtendedDiagnosticSession),
            &[0x10, 0x02],
            supports_ecu,
            ecu_access,
        );
        assert!(matches!(st, Stage::Proceed { sid: 0x10, .. }), "{st:?}");
        let st = begin(
            &in_session(S::ProgrammingSession),
            &[0x10, 0x02],
            supports_ecu,
            ecu_access,
        );
        assert!(
            matches!(
                st,
                Stage::Settle {
                    nrc: N::SubFunctionNotSupportedInActiveSession,
                    ..
                }
            ),
            "{st:?}"
        );
    }

    /// Annex A — 0x7E "shall only be used when the requested `SubFunction` is known to be
    /// supported in another session, otherwise 0x12 shall be used". A reserved session
    /// byte is 0x12 even where the per-session check would refuse it.
    #[test]
    fn a_reserved_session_byte_settles_0x12_not_0x7e() {
        for byte in [0x00, 0x05, 0x7F] {
            let request = [0x10, byte];
            let st = begin(
                &in_session(S::ExtendedDiagnosticSession),
                &request,
                supports_ecu,
                ecu_access_nowhere,
            );
            assert!(
                matches!(
                    st,
                    Stage::Settle {
                        sid: 0x10,
                        nrc: N::SubFunctionNotSupported
                    }
                ),
                "10 {byte:02X}: {st:?}"
            );
        }
    }

    /// `TesterPresent`'s zero sub-function proceeds in every session: clause 10.7 makes it
    /// the service's only one and Table 23 allows the service in every session, so row 4
    /// has nothing to refuse.
    #[test]
    fn tester_present_is_supported_in_every_session() {
        for session in [S::DefaultSession, S::ExtendedDiagnosticSession] {
            let st = begin(
                &in_session(session),
                &[0x3E, 0x00],
                supports_ecu,
                ecu_access,
            );
            assert!(matches!(st, Stage::Proceed { sid: 0x3E, .. }), "{st:?}");
        }
    }

    /// ``UDSSVC_ARCH_0009`` rule 1 — a 0x7E settled for a functionally addressed request
    /// is silenced; physically addressed it is written `7F 10 7E`.
    #[test]
    fn a_functional_0x7e_is_silenced() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let code = N::SubFunctionNotSupportedInActiveSession;
        let r = settle(
            ai(TaType::Functional),
            Unsettled::handled(sid(0x10), Err(code)),
            false,
            &mut out,
        );
        assert_eq!(r, Responded::Suppressed { session: None });
        assert_eq!(out.written_bytes(), &[0_u8; 0]);
        let r = settle(
            ai(TaType::Physical),
            Unsettled::handled(sid(0x10), Err(code)),
            false,
            &mut out,
        );
        assert_eq!(r, Responded::Yes { session: None });
        assert_eq!(out.written_bytes(), &[0x7F, 0x10, 0x7E]);
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
        let r = settle(
            ai(TaType::Physical),
            Unsettled::handled(sid(0x22), Err(N::RequestOutOfRange)),
            false,
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
        let r = settle(
            ai(TaType::Functional),
            Unsettled::handled(sid(0x22), Err(N::RequestOutOfRange)),
            false,
            &mut out,
        );
        assert_eq!(r, Responded::Suppressed { session: None });
        let r = settle(
            ai(TaType::Functional),
            Unsettled::handled(sid(0x22), Err(N::RequestOutOfRange)),
            true,
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
        let bit = Settling {
            sid: 0x10,
            suppress_bit: true,
        };
        let r = settle(
            ai(TaType::Physical),
            Unsettled::handled(bit, Ok(Some(S::ExtendedDiagnosticSession))),
            false,
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
            Unsettled::handled(bit, Err(N::SubFunctionNotSupported)),
            false,
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
        let r = settle(
            ai(TaType::Physical),
            Unsettled::handled(sid(0x22), Ok(None)),
            false,
            &mut out,
        );
        assert_eq!(r, Responded::Yes { session: None });
        assert_eq!(out.written_bytes(), &[0x7F, 0x22, 0x14]);
    }

    /// `settle`: rule 3 on the positive path — after a sent 0x78 the suppress bit no
    /// longer silences, and the session still rides on the response.
    #[test]
    fn a_sent_response_pending_overrides_the_suppress_bit() {
        let mut buf = [0_u8; 8];
        let mut out = ResponseSink::new(&mut buf, None);
        let _ = out.write_all(&[0x50, 0x03, 0, 50, 1, 244]);
        let bit = Settling {
            sid: 0x10,
            suppress_bit: true,
        };
        let outcome = Ok(Some(S::ExtendedDiagnosticSession));
        let r = settle(
            ai(TaType::Physical),
            Unsettled::handled(bit, outcome),
            true,
            &mut out,
        );
        assert_eq!(
            r,
            Responded::Yes {
                session: Some(S::ExtendedDiagnosticSession)
            }
        );
        assert_eq!(out.written_bytes(), &[0x50, 0x03, 0, 50, 1, 244]);
    }

    /// ``UDSSVC_ARCH_0005`` — a request with no service identifier is complete with no
    /// response, and no rule reaches it: not even a sent 0x78 makes it answer.
    #[test]
    fn an_empty_request_settles_silent_whatever_was_sent() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        for pending_sent in [false, true] {
            let r = settle(
                ai(TaType::Physical),
                Unsettled::empty(),
                pending_sent,
                &mut out,
            );
            assert_eq!(r, Responded::Suppressed { session: None });
            assert_eq!(out.written_bytes(), &[0_u8; 0]);
        }
    }

    /// A code the common stages settled is a negative outcome with no suppress bit:
    /// silenced functionally under rule 1, written physically.
    #[test]
    fn a_refusal_from_begin_settles_as_a_negative_outcome() {
        let mut buf = [0_u8; 4];
        let mut out = ResponseSink::new(&mut buf, None);
        let refused = Unsettled::refused(0x11, N::ServiceNotSupported);
        let r = settle(ai(TaType::Functional), refused, false, &mut out);
        assert_eq!(r, Responded::Suppressed { session: None });
        let r = settle(ai(TaType::Physical), refused, false, &mut out);
        assert_eq!(r, Responded::Yes { session: None });
        assert_eq!(out.written_bytes(), &[0x7F, 0x11, 0x11]);
    }
}
