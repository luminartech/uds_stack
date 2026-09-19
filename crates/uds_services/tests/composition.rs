//! Does the design fit together?
//!
//! Every body in this crate is `todo!()`, so nothing here calls one. What is proven is
//! composition: that an application can implement the traits, that `uds_server!` folds its
//! declared maxima into buffer lengths, and that the result constructs in a `static`
//! without a stack temporary.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "the fixtures below never await anything, since they exist only to name \
              concrete types for the traits' associated constants — not a real defect \
              in test code"
)]

use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, Ai, ClearDiagnosticInformation, CommunicationControl, CommunicationControlType,
    CommunicationType, DataIdentifier, DataTransfer, DtcReportKind, DtcStatusMask,
    FunctionalGroupIdentifier, KeyVerdict, Mtype, ReadDataByIdentifier,
    ReadDtcInfoSubFunction, ReadDtcInformation, RecordError, Reloads, ResponseSink,
    SecurityAccess, SecurityLevel, SecurityPolicy, ServerParams, ServiceSet, Sink, Storage,
    SubnetNumber, TaType, Timestamp, TransferRequest, TransportEvent, UdsTransport,
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
         ClearDiagnosticInformation, CommunicationControl;
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
    assert!(ecu.supports(0x22));
    assert!(ecu.supports(0x27));
    assert!(ecu.supports(0x36));
    assert!(ecu.supports(0x19));
    assert!(ecu.supports(0x14));
    assert!(ecu.supports(0x28));
    assert!(!ecu.supports(0x2E));
    assert!(!ecu.supports(0x85));
}

/// ``UDSSVC_ARCH_0033`` — Annex A permission is per service and declared.
#[test]
fn response_pending_permission_follows_the_declaration() {
    let ecu = Ecu::new();
    assert!(ecu.may_respond_pending(0x34));
    assert!(!ecu.may_respond_pending(0x22));
    assert!(!ecu.may_respond_pending(0x2E));
}

/// Clause 8.7.6's first exception is a **functionally addressed** suppressed
/// `TesterPresent`. The same bytes physically addressed are not admitted, which is the
/// distinction the classifier could not draw while its only argument was the request.
///
/// The *second* exception, a request in 0x00-0x0F, is unreachable for any server
/// `uds_server!` can assemble -- that range is OBD territory, which `uds_protocol` does
/// not model, so the minimum SID in `__uds_sids!` is 0x10. The 0x01 assertion is a guard
/// that the limb stays closed while nothing can open it, not a demonstration that it
/// works.
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

