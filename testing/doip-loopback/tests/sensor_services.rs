//! Every service the sensor stages, driven from a `uds_client!` client over loopback:
//! the sensor's own application, answering what the SDK will ask of it.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test harness: a failed expectation is the test failing"
)]

use doip_loopback::{
    ENTITY, Loopback, SensorClientTransport, SensorEntity, on_loopback, serve_until,
    ticking,
};
use embassy_time::{Duration, Instant};
use sensor_services::{DTCS, Did, Rid};
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_services::{
    Address, CLEAR_ALL_DTCS, ClientTiming, CommunicationControlType, CommunicationType,
    DataFormatIdentifier, DataIdentifier, DiagnosticSessionType as S, DtcRecord,
    DtcSettingType, DtcStatusMask, KeepAlive, NegativeResponseCode as Nrc,
    PhysicalKeepAlive, ReadDtcInfoResponse, ReadDtcInfoSubFunction, RecordError, Reloads,
    Replaced, ResetType, Response, RoutineControlSubFunction, SecurityLevel, ServerParams,
    Stopped, SubnetNumber, Timestamp, uds_client, uds_server,
};

sensor_services::ecu!();

uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = DoIpTransport<SensorEntity<4096>, 2>,
    peers = 1,
    server = SensorServer,
}

/// The sensor's identifiers as the client names them: `uds_client!` implements its
/// `ClientSet` for the identifier type, which must therefore be local to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SdkDid(Did);

impl DataIdentifier for SdkDid {
    const MAX_RECORD_LEN: usize = Did::MAX_RECORD_LEN;
    fn as_u16(self) -> u16 {
        self.0.as_u16()
    }
    fn from_u16(value: u16) -> Option<Self> {
        Did::from_u16(value).map(Self)
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        self.0.split_record(buf)
    }
}

uds_client! {
    SdkDid;
    transport = SensorClientTransport,
    max_dids_per_request = 4,
    routines = Rid,
    max_dtcs = 8,
    max_dtc_record_len = 32,
    max_seed_len = 2,
    max_key_len = 2,
    max_option_len = 4,
    max_block_len = 64,
    physical = 1,
    functional = 1,
    responders = 1,
    keep_alive = PhysicalKeepAlive,
    client = Sdk,
}

const SERVER: Address = Address(ENTITY.0);
const TESTER: Address = Address(doip_loopback::TESTER.0);
const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};
const RELOADS: Reloads = Reloads {
    default_reload: 100,
    enhanced_reload: 5_000,
};

/// How often the client sends its physical keep-alive, `TesterPresent`, in ms.
const KEEP_ALIVE_MS: u32 = 2_000;

/// How far [`ticking`] moves the clock at a time.
const STEP: Duration = Duration::from_millis(20);

fn server(loopback: &Loopback) -> SensorServer {
    SensorServer::new(
        Ecu::new(),
        DoIpTransport::new(loopback.entity(), bench_reloads()),
        SERVER,
        PARAMS,
    )
}

async fn sdk(loopback: &Loopback) -> Sdk {
    let transport = loopback
        .client_transport(Duration::from_ticks(0), RELOADS)
        .await
        .expect("activated");
    Sdk::new(
        transport,
        TESTER,
        KeepAlive::physical(KEEP_ALIVE_MS),
        ClientTiming::new(0, 0, 0),
    )
}

fn level(seed: u8) -> SecurityLevel {
    SecurityLevel::from_request_seed(seed).expect("a level")
}

/// Enters the extended session, where ISO 14229-1:2020 10.2 Table 23 admits the
/// services the pipeline refuses in the default one.
async fn extended(c: &mut Sdk) {
    let entered = c
        .diagnostic_session_control(SERVER, S::ExtendedDiagnosticSession)
        .await;
    assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
}

/// `ms` from now on the mock clock, which the tester's clock reads.
fn in_ms(ms: u64) -> Timestamp {
    let at = Instant::now()
        .checked_add(Duration::from_millis(ms))
        .expect("a test's clock stays far from overflow");
    Timestamp(u32::try_from(at.as_millis()).expect("a test's clock stays below 2^32 ms"))
}

/// Runs `test` with the sensor served and a client connected.
fn with_sensor<F, Fut>(test: F)
where
    F: FnOnce(Sdk) -> Fut + Send + 'static,
    Fut: core::future::Future<Output = ()>,
{
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let client = sdk(&loopback).await;
            test(client).await;
        })
        .await;
    });
}

#[test]
fn a_write_in_the_right_session_is_positive_and_a_forbidden_one_negative() {
    with_sensor(|mut c| async move {
        let written = c
            .write_data_by_identifier(SERVER, SdkDid(Did::Mode), &[0x01])
            .await;
        assert_eq!(written.expect("answered"), Response::Positive(()));
        let read = c
            .read_data_by_identifier(SERVER, &[SdkDid(Did::Mode)])
            .await;
        match read {
            Ok(Response::Positive(mut records)) => {
                assert_eq!(records.next(), Some((SdkDid(Did::Mode), &[0x01][..])));
            }
            other => panic!("{other:?}"),
        }
        let forbidden = c
            .write_data_by_identifier(SERVER, SdkDid(Did::VehicleSpeed), &[0x00])
            .await;
        assert_eq!(
            forbidden.expect("answered"),
            Response::Negative(Nrc::RequestOutOfRange)
        );
        c.close().await.expect("closed");
    });
}

#[test]
fn a_seed_then_the_right_key_unlocks_and_a_wrong_key_is_refused() {
    with_sensor(|mut c| async move {
        extended(&mut c).await;
        let seed = match c.security_access_seed(SERVER, level(0x01)).await {
            Ok(Response::Positive(&[high, low])) => u16::from_be_bytes([high, low]),
            other => panic!("{other:?}"),
        };
        assert_eq!(seed, 0x3657);
        let wrong = c
            .security_access_key(SERVER, level(0x01), &[0x00, 0x00])
            .await;
        assert_eq!(
            wrong.expect("answered"),
            Response::Negative(Nrc::InvalidKey)
        );
        let seed = match c.security_access_seed(SERVER, level(0x01)).await {
            Ok(Response::Positive(&[high, low])) => u16::from_be_bytes([high, low]),
            other => panic!("{other:?}"),
        };
        let right = c
            .security_access_key(SERVER, level(0x01), &seed.wrapping_neg().to_be_bytes())
            .await;
        assert_eq!(right.expect("answered"), Response::Positive(()));
        c.close().await.expect("closed");
    });
}

#[test]
fn a_routine_is_started_and_its_results_read() {
    with_sensor(|mut c| async move {
        let started = c
            .routine_control(
                SERVER,
                RoutineControlSubFunction::StartRoutine,
                Rid::SelfTest,
                &[0x01],
            )
            .await;
        assert_eq!(started.expect("answered"), Response::Positive(&[0x00][..]));
        let results = c
            .routine_control(
                SERVER,
                RoutineControlSubFunction::RequestRoutineResults,
                Rid::SelfTest,
                &[],
            )
            .await;
        assert_eq!(
            results.expect("answered"),
            Response::Positive(&[0x00, 0x5A][..])
        );
        let stopped = c
            .routine_control(
                SERVER,
                RoutineControlSubFunction::StopRoutine,
                Rid::SelfTest,
                &[],
            )
            .await;
        assert_eq!(
            stopped.expect("answered"),
            Response::Negative(Nrc::SubFunctionNotSupported)
        );
        c.close().await.expect("closed");
    });
}

#[test]
fn a_reset_is_answered_and_its_close_survived() {
    with_sensor(|mut c| async move {
        let reset = c
            .ecu_reset(SERVER, ResetType::EnableRapidPowerShutDown)
            .await;
        assert_eq!(reset.expect("answered"), Response::Positive(Some(0x0A)));
        let again = c.ecu_reset(SERVER, ResetType::HardReset).await;
        assert_eq!(again.expect("answered"), Response::Positive(None));
        let read = c
            .read_data_by_identifier(SERVER, &[SdkDid(Did::Mode)])
            .await;
        assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
        c.close().await.expect("closed");
    });
}

#[test]
fn dtcs_are_counted_listed_and_cleared() {
    with_sensor(|mut c| async move {
        let counted = c
            .read_dtc_information(
                SERVER,
                ReadDtcInfoSubFunction::ReportNumberOfDtcByStatusMask(DtcStatusMask::from(
                    0xFF,
                )),
            )
            .await;
        assert!(
            matches!(
                counted,
                Ok(Response::Positive(ReadDtcInfoResponse::NumberOfDtcs {
                    count: 3,
                    ..
                }))
            ),
            "{counted:?}"
        );
        let listed = c
            .read_dtc_information(
                SERVER,
                ReadDtcInfoSubFunction::ReportDtcByStatusMask(DtcStatusMask::from(0xFF)),
            )
            .await;
        match listed {
            Ok(Response::Positive(ReadDtcInfoResponse::DtcList {
                raw_records, ..
            })) => {
                assert_eq!(raw_records.len(), 4 * DTCS.len());
            }
            other => panic!("{other:?}"),
        }
        let cleared = c
            .clear_diagnostic_information(SERVER, CLEAR_ALL_DTCS, None)
            .await;
        assert_eq!(cleared.expect("answered"), Response::Positive(()));
        let refused = c
            .clear_diagnostic_information(SERVER, DtcRecord::new(0x12, 0x34, 0x56), None)
            .await;
        assert_eq!(
            refused.expect("answered"),
            Response::Negative(Nrc::RequestOutOfRange)
        );
        c.close().await.expect("closed");
    });
}

#[test]
fn supported_dtcs_need_the_extended_session() {
    with_sensor(|mut c| async move {
        let outside = c
            .read_dtc_information(SERVER, ReadDtcInfoSubFunction::ReportSupportedDtc)
            .await;
        assert_eq!(
            outside.expect("answered"),
            Response::Negative(Nrc::SubFunctionNotSupportedInActiveSession)
        );
        let entered = c
            .diagnostic_session_control(SERVER, S::ExtendedDiagnosticSession)
            .await;
        assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
        let inside = c
            .read_dtc_information(SERVER, ReadDtcInfoSubFunction::ReportSupportedDtc)
            .await;
        assert!(
            matches!(
                inside,
                Ok(Response::Positive(ReadDtcInfoResponse::DtcList { .. }))
            ),
            "{inside:?}"
        );
        c.close().await.expect("closed");
    });
}

#[test]
fn communication_and_dtc_setting_are_controlled() {
    with_sensor(|mut c| async move {
        extended(&mut c).await;
        let enabled = c
            .communication_control(
                SERVER,
                CommunicationControlType::EnableRxAndTx,
                CommunicationType::Normal,
                SubnetNumber::AllConnectedNetworks,
                None,
            )
            .await;
        assert_eq!(enabled.expect("answered"), Response::Positive(()));
        let refused = c
            .communication_control(
                SERVER,
                CommunicationControlType::EnableRxAndDisableTxWithEnhancedAddressInfo,
                CommunicationType::Normal,
                SubnetNumber::AllConnectedNetworks,
                Some(0xFFFF),
            )
            .await;
        assert_eq!(
            refused.expect("answered"),
            Response::Negative(Nrc::RequestOutOfRange)
        );
        let on = c.control_dtc_setting(SERVER, DtcSettingType::On, &[]).await;
        assert_eq!(on.expect("answered"), Response::Positive(()));
        let bad = c
            .control_dtc_setting(SERVER, DtcSettingType::Off, &[0xFF])
            .await;
        assert_eq!(
            bad.expect("answered"),
            Response::Negative(Nrc::RequestOutOfRange)
        );
        c.close().await.expect("closed");
    });
}

#[test]
fn the_prescribed_close_is_reported_as_a_replacement_once() {
    with_sensor(|mut c| async move {
        let none = c.connection_replaced(SERVER).await;
        assert_eq!(none.expect("asked"), Replaced::No);
        for session in [S::ExtendedDiagnosticSession, S::ProgrammingSession] {
            let changed = c.diagnostic_session_control(SERVER, session).await;
            assert!(matches!(changed, Ok(Response::Positive(_))), "{changed:?}");
        }
        // The close is seen once the transport has been polled after it arrived.
        ticking(c.idle_until(in_ms(500)), STEP)
            .await
            .expect("idled");
        let left = c.connection_replaced(SERVER).await;
        assert_eq!(left.expect("asked"), Replaced::SessionLost);
        let again = c.connection_replaced(SERVER).await;
        assert_eq!(again.expect("asked"), Replaced::No);
        c.close().await.expect("closed");
    });
}

/// A positive reset is reported as a replacement at once, with nothing between: the
/// exchange itself reports it, before the transport reports the close. The close the
/// transport reports later, by the time the next call has reconnected, is that same
/// replacement, not a second one.
#[test]
fn a_positive_reset_is_reported_as_a_replacement_at_once() {
    with_sensor(|mut c| async move {
        let reset = c.ecu_reset(SERVER, ResetType::HardReset).await;
        assert_eq!(reset.expect("answered"), Response::Positive(None));
        let left = c.connection_replaced(SERVER).await;
        assert_eq!(left.expect("asked"), Replaced::SessionLost);
        let again = c.connection_replaced(SERVER).await;
        assert_eq!(again.expect("asked"), Replaced::No);
        let read = c
            .read_data_by_identifier(SERVER, &[SdkDid(Did::Mode)])
            .await;
        assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
        let after = c.connection_replaced(SERVER).await;
        assert_eq!(after.expect("asked"), Replaced::No);
        c.close().await.expect("closed");
    });
}

/// The keep-alive is the `TesterPresent` the client sends: idling in the extended
/// session past several keep-alive intervals and past the server's S3, the session
/// survives, so a service the default session refuses is still answered positively.
#[test]
fn the_keep_alive_holds_the_extended_session_past_s3() {
    with_sensor(|mut c| async move {
        extended(&mut c).await;
        let idle = u64::from(PARAMS.s3_server) + 1_000;
        assert!(idle > 2 * u64::from(KEEP_ALIVE_MS));
        ticking(c.idle_until(in_ms(idle)), STEP)
            .await
            .expect("idled");
        let inside = c
            .read_dtc_information(SERVER, ReadDtcInfoSubFunction::ReportSupportedDtc)
            .await;
        assert!(
            matches!(
                inside,
                Ok(Response::Positive(ReadDtcInfoResponse::DtcList { .. }))
            ),
            "{inside:?}"
        );
        c.close().await.expect("closed");
    });
}

/// 15.2, 15.4 and 15.5 — the SDK's client downloads 80 bytes to the sensor in the
/// programming session, level 0x03 unlocked, in blocks of `maxNumberOfBlockLength - 2`;
/// the sensor's handler receives exactly the bytes sent.
#[test]
fn the_client_downloads_to_the_sensor() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        let image: Vec<u8> = (0..80_u8).collect();
        serve_until(&mut server, async {
            let mut c = sdk(&loopback).await;
            extended(&mut c).await;
            let entered = c
                .diagnostic_session_control(SERVER, S::ProgrammingSession)
                .await;
            assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
            let seed = match c.security_access_seed(SERVER, level(0x03)).await {
                Ok(Response::Positive(&[high, low])) => u16::from_be_bytes([high, low]),
                other => panic!("{other:?}"),
            };
            let key = c
                .security_access_key(
                    SERVER,
                    level(0x03),
                    &seed.wrapping_neg().to_be_bytes(),
                )
                .await;
            assert_eq!(key.expect("answered"), Response::Positive(()));
            let advertised = match c
                .request_download(SERVER, DataFormatIdentifier::NONE, 0x0010_0000, 80)
                .await
            {
                Ok(Response::Positive(length)) => length,
                other => panic!("{other:?}"),
            };
            assert_eq!(advertised, 0x22);
            let block = usize::try_from(advertised - 2).expect("small");
            for (n, chunk) in image.chunks(block).enumerate() {
                let counter = u8::try_from(n + 1).expect("few blocks");
                let sent = c.transfer_data(SERVER, counter, chunk).await;
                assert!(matches!(sent, Ok(Response::Positive(_))), "{sent:?}");
            }
            let exit = c.request_transfer_exit(SERVER, &[]).await;
            assert!(matches!(exit, Ok(Response::Positive(_))), "{exit:?}");
            c.close().await.expect("closed");
        })
        .await;
        let sum = image
            .iter()
            .fold(0_u32, |sum, &byte| sum.wrapping_add(u32::from(byte)));
        assert_eq!(server.services().downloaded(), (80, sum));
    });
}

/// 10.3, ISO 14229-5:2022 REQ 7.11 — a hard reset ends `run` once its response is
/// confirmed, after the transport closed the connection (the client sees the replacement
/// it prescribed), and the server stepped again serves the reconnected client.
#[test]
fn a_hard_reset_ends_run_after_the_close() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        let client = async {
            let mut c = sdk(&loopback).await;
            let reset = c.ecu_reset(SERVER, ResetType::HardReset).await;
            assert_eq!(reset.expect("answered"), Response::Positive(None));
            c
        };
        let (stopped, mut c) = tokio::join!(server.run(), client);
        assert_eq!(
            stopped.expect("stopped"),
            Stopped::Reset(ResetType::HardReset)
        );
        serve_until(&mut server, async {
            let left = c.connection_replaced(SERVER).await;
            assert_eq!(left.expect("asked"), Replaced::SessionLost);
            let read = c
                .read_data_by_identifier(SERVER, &[SdkDid(Did::Mode)])
                .await;
            assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
            c.close().await.expect("closed");
        })
        .await;
    });
}
