//! Does the design fit together?
//!
//! What is proven is composition: that an application can implement the traits, that
//! `uds_server!` folds its declared maxima into buffer lengths, that the result
//! constructs in a `static` without a stack temporary, and that the `dispatch` it emits
//! routes a request through the pipeline.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "the fixtures below never await anything, since they exist only to name \
              concrete types for the traits' associated constants — not a real defect \
              in test code"
)]

use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, Ai, Answer, ClearDiagnosticInformation, ClientSet, ClientStorage,
    CommunicationControl, CommunicationControlType, CommunicationType, DataIdentifier,
    DataTransfer, DiagnosticSessionType, DtcReportKind, DtcStatusMask,
    FunctionalGroupIdentifier, KeyVerdict, Mtype, PhysicalKeepAlive, ReadDataByIdentifier,
    ReadDtcInfoSubFunction, ReadDtcInformation, RecordError, Reloads, Response,
    ResponseSink, SecurityAccess, SecurityLevel, SecurityPolicy, ServerParams, ServiceSet,
    SessionTiming, SessionTransition, Sink, Storage, SubnetNumber, TaType, TesterPresent,
    Timestamp, TransferRequest, TransportEvent, UdsServiceType, UdsTransport, uds_client,
    uds_server,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    VehicleSpeed,
    VinNumber,
}

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 17;
    fn as_u16(self) -> u16 {
        match self {
            Self::VehicleSpeed => 0xF4_0D,
            Self::VinNumber => 0xF1_90,
        }
    }
    fn from_u16(v: u16) -> Option<Self> {
        match v {
            0xF4_0D => Some(Self::VehicleSpeed),
            0xF1_90 => Some(Self::VinNumber),
            _ => None,
        }
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        let w = match self {
            Self::VehicleSpeed => 1,
            Self::VinNumber => 17,
        };
        buf.split_at_checked(w).ok_or(RecordError::Short)
    }
}

#[derive(Debug)]
struct Ecu {
    attempts: u8,
    delay: bool,
}
impl Ecu {
    const fn new() -> Self {
        Self {
            attempts: 0,
            delay: false,
        }
    }
}

impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 4;
    async fn read(&mut self, did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        match did {
            Did::VehicleSpeed => out.write_all(&[0x40]),
            Did::VinNumber => out.write_all(&[0x00; 17]),
        }
        .map_err(|_| Nrc::ResponseTooLong)
    }
}

impl ReadDtcInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DTCS: usize = 10;
    const REPORTS: &'static [DtcReportKind] =
        &[DtcReportKind::DtcList, DtcReportKind::SeverityList];
    // No `parameters: &[u8]`: every report type's parameters ride on its variant.
    async fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        _out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        match request {
            ReadDtcInfoSubFunction::ReportDtcByStatusMask(_mask) => Ok(()),
            _ => Err(Nrc::SubFunctionNotSupported),
        }
    }
}

impl ClearDiagnosticInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = true;
    async fn clear(
        &mut self,
        group: FunctionalGroupIdentifier,
        _memory_selection: Option<u8>,
    ) -> Result<(), Nrc> {
        match group {
            FunctionalGroupIdentifier::EmissionsSystemGroup => Ok(()),
            _ => Err(Nrc::RequestOutOfRange),
        }
    }
}

impl TesterPresent for Ecu {
    // No `MAY_RESPOND_PENDING`: the trait does not carry one, because a synchronous
    // handler can never be in progress when a deadline passes.
    fn on_tester_present(&mut self) {}
}

impl CommunicationControl for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    // The two sub-function bytes are unrelated types, so they cannot be transposed.
    async fn control(
        &mut self,
        control_type: CommunicationControlType,
        communication_type: CommunicationType,
        node: SubnetNumber,
    ) -> Result<(), Nrc> {
        match (control_type, communication_type, node) {
            (
                CommunicationControlType::DisableRxAndTx,
                CommunicationType::NetworkManagement,
                SubnetNumber::ReceivedOn,
            ) => Err(Nrc::ConditionsNotCorrect),
            _ => Ok(()),
        }
    }
}

impl DataTransfer for Ecu {
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_BLOCK_LENGTH: usize = 1_024;
    const SUPPORTS_UPLOAD: bool = false;
    async fn begin(&mut self, _r: TransferRequest<'_>) -> Result<(), Nrc> {
        Ok(())
    }
    async fn block(&mut self, _d: &[u8], _o: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        Ok(())
    }
    async fn exit(&mut self, _r: &[u8], _o: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        Ok(())
    }
}

impl SecurityAccess for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_SEED_LEN: usize = 4;
    const MAX_KEY_LEN: usize = 4;
    fn policy(&self, _l: SecurityLevel) -> SecurityPolicy {
        SecurityPolicy::Counted {
            attempt_limit: 3,
            delay_ms: Some(10_000),
            static_seed: false,
        }
    }
    fn load_attempts(&self, _l: SecurityLevel) -> u8 {
        self.attempts
    }
    fn store_attempts(&mut self, _l: SecurityLevel, c: u8) {
        self.attempts = c;
    }
    fn delay_running(&self, _l: SecurityLevel) -> bool {
        self.delay
    }
    fn start_delay(&mut self, _l: SecurityLevel) {
        self.delay = true;
    }
    async fn seed(
        &mut self,
        _l: SecurityLevel,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        out.write_all(&[1, 2, 3, 4])
            .map_err(|_| Nrc::ResponseTooLong)
    }
    async fn verify_key(
        &mut self,
        _l: SecurityLevel,
        key: &[u8],
    ) -> Result<KeyVerdict, Nrc> {
        Ok(if key == [4, 3, 2, 1] {
            KeyVerdict::Valid
        } else {
            KeyVerdict::Invalid
        })
    }
}

// Named by path: `ServiceSet` is in scope, and both traits have a `supports`.
impl uds_services::DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, session: DiagnosticSessionType) -> bool {
        matches!(
            session,
            DiagnosticSessionType::DefaultSession
                | DiagnosticSessionType::ProgrammingSession
                | DiagnosticSessionType::ExtendedDiagnosticSession
        )
    }
    fn timing(&self, _s: DiagnosticSessionType) -> SessionTiming {
        SessionTiming {
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _relocked: bool) {}
}

#[derive(Debug)]
struct FakeTransport;

impl UdsTransport for FakeTransport {
    type Error = ();
    async fn t_data_req(&mut self, _ai: Ai, _d: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    async fn next_event<'b>(
        &mut self,
        _b: &'b mut [u8],
        _d: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, ()> {
        Ok(TransportEvent::Deadline)
    }
    fn outbound_max(&self) -> Option<usize> {
        None
    }
    fn channel_timing(&self) -> Reloads {
        Reloads {
            default_reload: 2_000,
            enhanced_reload: 5_000,
        }
    }
    fn now(&self) -> Timestamp {
        Timestamp(0)
    }
}

uds_server! {
    Ecu: ReadDataByIdentifier, SecurityAccess, DataTransfer, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, TesterPresent,
         DiagnosticSessionControl;
    transport = FakeTransport,
    peers = 4,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
};

/// The construction that matters: a multi-kilobyte server in a `static`, built in place
/// with no stack temporary. This is what `Storage::EMPTY` being an associated const buys.
static SERVER: EcuServer = EcuServer::new(Ecu::new(), FakeTransport, PARAMS);

/// The in-flight buffer is dominated by `TransferData` — the same constant the server
/// must advertise as `maxNumberOfBlockLength`. The response buffer is dominated by a
/// four-identifier read, because this server is download-only and never sends a block.
///
/// `ReadDTCInformation` is the other contender and loses: this server declares
/// [`DtcReportKind::SeverityList`], whose six-byte records make it the widest of the two
/// layouts it answers, for `3 + 10 * 6 = 63` against the read's 77. Under the
/// `DTC_RECORD_LEN` this replaced, the obvious declaration of `4` would have folded in
/// `3 + 10 * 4 = 43` and left a severity report twenty bytes short of its own buffer.
///
/// **If these fail, the macro's bound arms are wrong, not these numbers.** Read a failure
/// as the design being wrong about a bound.
#[test]
fn the_buffers_are_derived_from_the_declared_maxima() {
    let mut store = <<Ecu as ServiceSet>::Store as Storage>::EMPTY;
    let b = store.split();
    assert_eq!(b.in_flight.len(), 2 + 1_024);
    assert_eq!(b.concurrent.len(), 8);
    assert_eq!(b.response.len(), 1 + 4 * (2 + 17));
    // And nothing else: the session owns the associations, so the store holds only
    // the three buffers.
    assert_eq!(
        core::mem::size_of::<<Ecu as ServiceSet>::Store>(),
        (2 + 1_024) + 8 + (1 + 4 * (2 + 17))
    );
}

/// ``UDSSVC_ARCH_0006`` — `serviceNotSupported` is decided from the assembly list.
#[test]
fn the_assembled_list_answers_service_supported() {
    let ecu = Ecu::new();
    assert!(ecu.supports(UdsServiceType::ReadDataByIdentifier));
    assert!(ecu.supports(UdsServiceType::SecurityAccess));
    assert!(ecu.supports(UdsServiceType::TransferData));
    assert!(ecu.supports(UdsServiceType::ReadDtcInfo));
    assert!(ecu.supports(UdsServiceType::ClearDiagnosticInfo));
    assert!(ecu.supports(UdsServiceType::CommunicationControl));
    assert!(ecu.supports(UdsServiceType::TesterPresent));
    assert!(!ecu.supports(UdsServiceType::WriteDataByIdentifier));
    assert!(!ecu.supports(UdsServiceType::ControlDtcSetting));
    // A byte naming no service at all resolves to one variant, which no list contains.
    assert!(!ecu.supports(UdsServiceType::from_request_sid(0x01)));
}

/// ``UDSSVC_ARCH_0033`` — Annex A permission is per service and declared.
#[test]
fn response_pending_permission_follows_the_declaration() {
    let ecu = Ecu::new();
    assert!(ecu.may_respond_pending(UdsServiceType::RequestDownload));
    assert!(!ecu.may_respond_pending(UdsServiceType::ReadDataByIdentifier));
    assert!(!ecu.may_respond_pending(UdsServiceType::WriteDataByIdentifier));
    // 0x3E is false because the trait carries no constant to declare otherwise, not
    // because this server declared it so. Nothing an application writes can flip it.
    assert!(!ecu.may_respond_pending(UdsServiceType::TesterPresent));
}

/// The one message clause 8.7.6 admits mid-service is a **functionally addressed**
/// suppressed `TesterPresent`. The same bytes physically addressed are not admitted,
/// which is the distinction the classifier could not draw while its only argument was
/// the request.
///
/// The 0x01 assertion covers the clause's other exception, a request in 0x00-0x0F. No
/// server `uds_server!` can assemble reaches it — the lowest SID in `__uds_sids!` is
/// 0x10, because that range is OBD territory and `uds_protocol` does not model it — so
/// what this pins is that such a request is ordinary occupancy here, owing
/// busyRepeatRequest like any other.
#[test]
fn the_tester_present_exception_is_admitted_only_when_functionally_addressed() {
    fn ai(ta_type: TaType) -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0E00),
            ta_type,
        }
    }

    let ecu = Ecu::new();
    assert!(ecu.is_concurrent_exception(&[0x3E, 0x80], ai(TaType::Functional)));
    assert!(!ecu.is_concurrent_exception(&[0x3E, 0x80], ai(TaType::Physical)));
    // Without the suppress bit it is an ordinary request either way.
    assert!(!ecu.is_concurrent_exception(&[0x3E, 0x00], ai(TaType::Functional)));
    // An OBD-range service identifier: no exception, because none is assemblable.
    assert!(!ecu.is_concurrent_exception(&[0x01], ai(TaType::Functional)));
    assert!(!ecu.is_concurrent_exception(&[0x22, 0xF1, 0x90], ai(TaType::Functional)));
}

/// Referencing the static is what forces the const evaluation to run.
#[test]
fn the_server_constructs_in_a_static() {
    assert!(!core::ptr::addr_of!(SERVER).is_null());
}

/// The handler seam speaks `uds_protocol`'s vocabulary, so a sub-function is a named
/// value rather than a byte. The two `CommunicationControl` sub-function bytes were the
/// hazard this retyping removes: as `u8`s they sat adjacent in the signature and swapping
/// them compiled, and as unrelated enums the swap is a type error.
///
/// `ReadDtcInformation` is the other half of the change: its report type carries its own
/// parameters, so the handler no longer receives — or has to parse — a `&[u8]` beside it.
#[test]
fn the_handler_seam_is_typed_not_byte_shaped() {
    fn poll_once<F: Future>(mut f: core::pin::Pin<&mut F>) -> Option<F::Output> {
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        match f.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(v) => Some(v),
            core::task::Poll::Pending => None,
        }
    }

    let mut ecu = Ecu::new();

    let denied = poll_once(core::pin::pin!(ecu.control(
        CommunicationControlType::DisableRxAndTx,
        CommunicationType::NetworkManagement,
        SubnetNumber::ReceivedOn,
    )));
    assert_eq!(denied, Some(Err(Nrc::ConditionsNotCorrect)));

    // `SubnetNumber` distinguishes these two, where the old `Option<u16>` could not.
    assert_ne!(SubnetNumber::ReceivedOn, SubnetNumber::AllConnectedNetworks);

    let mut buffer = [0_u8; 8];
    let mut sink = ResponseSink::new(&mut buffer, None);
    let report = ReadDtcInfoSubFunction::ReportDtcByStatusMask(DtcStatusMask::TestFailed);
    assert_eq!(
        poll_once(core::pin::pin!(ecu.read_dtc_information(report, &mut sink))),
        Some(Ok(()))
    );

    let cleared = poll_once(core::pin::pin!(
        ecu.clear(FunctionalGroupIdentifier::EmissionsSystemGroup, None)
    ));
    assert_eq!(cleared, Some(Ok(())));
}

uds_client! {
    Did;
    transport = FakeTransport,
    max_dids_per_request = 4,
    physical = 2,
    functional = 1,
    responders = 4,
    keep_alive = PhysicalKeepAlive,
    client = Tester,
}

/// A client constructs in a `static` for the same reason the server above does, and
/// without the application naming `uds_session` to supply channel slots.
static TESTER: Tester = Tester::new(FakeTransport, PhysicalKeepAlive);

/// ``UDSSVC_ARCH_0024`` — the tester and the server size the same exchange from the same
/// `Did` declaration. The request is the service identifier and two bytes per identifier;
/// the response is the service identifier and each identifier beside its record, which is
/// the same `1 + 4 * (2 + 17)` the server folded in for `ReadDataByIdentifier`.
///
/// **If these disagree, one side of the vocabulary moved without the other.** That is the
/// compile error one identifier catalogue serving both roles exists to produce.
#[test]
fn the_client_buffers_are_derived_from_the_same_declaration() {
    let mut store = <<Did as ClientSet>::Store as ClientStorage>::EMPTY;
    let b = store.split();
    assert_eq!(b.request.len(), 1 + 2 * 4);
    assert_eq!(b.response.len(), 1 + 4 * (2 + 17));

    let mut server_store = <<Ecu as ServiceSet>::Store as Storage>::EMPTY;
    assert_eq!(b.response.len(), server_store.split().response.len());
}

/// Referencing the static is what forces the client's const evaluation to run.
#[test]
fn the_client_constructs_in_a_static() {
    assert!(!core::ptr::addr_of!(TESTER).is_null());
}

/// The point of the alias: a signature naming a client names *one* type, where before it
/// spelled a transport, a keep-alive mode and three counts.
///
/// It also pins the borrow this crate could not express until the client owned storage —
/// `Records<'_, Did>` borrows the response buffer out of `&mut Tester`. Never awaited, so
/// the `todo!()` bodies are compiled and not run: compile-time assertions in a test's
/// clothes, as `transport.rs`'s borrow test is.
///
/// What they assert beyond the borrow is the shape of a read. Reaching a record is a
/// `match` and a `for`: the walk yields pairs rather than `Result`s, because the response
/// was checked when it was built.
#[expect(dead_code, reason = "compiled for its signature, never called")]
async fn read_the_vin(tester: &mut Tester) -> Result<&'static str, ()> {
    Ok(
        match tester
            .read_data_by_identifier(Address(0x0E00), &[Did::VinNumber])
            .await?
        {
            Response::Positive(records) => {
                for (did, record) in records {
                    let _ = (did, record);
                }
                "read"
            }
            Response::Negative(code) => {
                let _ = code;
                "declined"
            }
            Response::Malformed(error) => {
                let _ = error;
                "unreadable"
            }
            Response::NoResponseExpected => "suppressed",
        },
    )
}

/// The functional path, which is where the depth was worst: an answer is one `match`, and
/// every case names the server that gave it. There is no "nothing came back" arm, because
/// a silent server produces no answer at all — the window closing is `None`.
#[expect(dead_code, reason = "compiled for its signature, never called")]
async fn read_the_vin_from_every_server(tester: &mut Tester) -> Result<(), ()> {
    let mut answers =
        tester.read_data_by_identifier_functional(Address(0x0E00), &[Did::VehicleSpeed]);
    while let Some(answer) = answers.next().await {
        match answer? {
            Answer::Positive { from, records } => {
                for (did, record) in records {
                    let _ = (from, did, record);
                }
            }
            Answer::Negative { from, code } => {
                let _ = (from, code);
            }
            Answer::Malformed { from, error } => {
                let _ = (from, error);
            }
        }
    }
    Ok(())
}

#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
fn block_on<F: core::future::Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("milestone-1 handlers never pend"),
    }
}

/// ``UDSSVC_ARCH_0004`` through the assembled entry point: a read answers positively.
#[test]
fn the_assembled_dispatch_answers_a_read() {
    let mut ecu = Ecu::new();
    let mut state = <<Ecu as ServiceSet>::State as uds_services::ProtocolState>::INITIAL;
    let mut buf = [0_u8; 32];
    let mut out = ResponseSink::new(&mut buf, None);
    let ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0E80),
        ta: Address(0x10),
        ta_type: TaType::Physical,
    };
    let no = core::sync::atomic::AtomicBool::new(false);
    let r = block_on(ecu.dispatch(&mut state, ai, &[0x22, 0xF4, 0x0D], &mut out, &no));
    assert_eq!(r, uds_services::Responded::Yes { session: None });
    assert_eq!(out.written_bytes(), &[0x62, 0xF4, 0x0D, 0x40]);
}

/// A listed service whose stage is not yet written, and an unlisted one, both settle
/// 0x11 — physically; functionally they are silenced (``UDSSVC_ARCH_0009`` rule 1).
#[test]
fn unsupported_services_settle_0x11_or_silence() {
    let mut ecu = Ecu::new();
    let mut state = <<Ecu as ServiceSet>::State as uds_services::ProtocolState>::INITIAL;
    let mut buf = [0_u8; 32];
    let mut out = ResponseSink::new(&mut buf, None);
    let phys = Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0E80),
        ta: Address(0x10),
        ta_type: TaType::Physical,
    };
    let no = core::sync::atomic::AtomicBool::new(false);
    let r = block_on(ecu.dispatch(&mut state, phys, &[0x11, 0x01], &mut out, &no));
    assert_eq!(r, uds_services::Responded::Yes { session: None });
    assert_eq!(out.written_bytes(), &[0x7F, 0x11, 0x11]);
    // Listed, so `begin` passes it; no stage, so the fall-through settles it.
    let r =
        block_on(ecu.dispatch(&mut state, phys, &[0x14, 0xFF, 0xFF, 0xFF], &mut out, &no));
    assert_eq!(r, uds_services::Responded::Yes { session: None });
    assert_eq!(out.written_bytes(), &[0x7F, 0x14, 0x11]);
    let func = Ai {
        ta_type: TaType::Functional,
        ..phys
    };
    let r = block_on(ecu.dispatch(&mut state, func, &[0x11, 0x01], &mut out, &no));
    assert_eq!(r, uds_services::Responded::Suppressed { session: None });
}
