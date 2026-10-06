//! Two testers on two connections of one entity, served by one server.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test harness: scripts are fixed and futures complete in bounded polls"
)]

#[allow(dead_code, reason = "each test binary uses part of the shared mock")]
mod support;

use core::future::Future;
use simple_doip::LogicalAddress;
use simple_doip::service::ConnectionId;
use support::{MockEntity, TESTER, Tester, Wire, block_on, exclusive_clock};
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, DataIdentifier, ReadDataByIdentifier, RecordError, ResponseSink, ServerParams,
    Sink, TesterPresent, uds_server,
};

const OTHER: LogicalAddress = LogicalAddress(0x0E01);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    Speed,
}
impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        0xF40D
    }
    fn from_u16(v: u16) -> Option<Self> {
        (v == 0xF40D).then_some(Self::Speed)
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        buf.split_at_checked(1).ok_or(RecordError::Short)
    }
}

/// Pends `self.0` times before completing, waking itself each time.
#[derive(Debug)]
struct PendN(u8);
impl Future for PendN {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.0 == 0 {
            return core::task::Poll::Ready(());
        }
        self.0 = self.0.saturating_sub(1);
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
    }
}

#[derive(Debug, Default)]
struct Ecu {
    /// How many times the next `read` pends before answering.
    slow: u8,
}
impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.slow)).await;
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}
impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

uds_server! {
    Ecu: ReadDataByIdentifier, TesterPresent;
    transport = DoIpTransport<MockEntity<2>, 2>,
    peers = 1,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

/// One tester leaving ends nothing of another's: tester B's slow read, in progress when
/// tester A's connection closes, is still answered on B's connection.
#[test]
fn one_tester_leaving_leaves_the_other_testers_request_in_progress() {
    let _clock = exclusive_clock();
    let mut s = EcuServer::new(
        Ecu::default(),
        DoIpTransport::new(
            MockEntity::new([
                Tester::Connects(TESTER),
                Tester::Connects(OTHER),
                Tester::Sends(TESTER, vec![0x3E, 0x00]),
                Tester::Sends(OTHER, vec![0x22, 0xF4, 0x0D]),
                Tester::Leaves(TESTER),
            ]),
            bench_reloads(),
        ),
        Address(0x0001),
        PARAMS,
    );
    s.services().slow = 3;
    while block_on(s.step()).is_ok() {}
    let entity = s.transport().entity();
    assert!(entity.script.is_empty(), "the run ended early");
    assert_eq!(
        entity.wire.last(),
        Some(&Wire::Data(
            ConnectionId::new(1),
            vec![0x62, 0xF4, 0x0D, 0x40]
        )),
        "{:?}",
        entity.wire
    );
}
