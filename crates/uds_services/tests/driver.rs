//! The driver's arms, against transports that script one situation each.
//!
//! An integration test rather than a unit test in `server.rs`: `uds_server!` cannot be
//! invoked inside this crate, because the helper macros it calls through `$crate::` are
//! themselves macro-expanded `macro_export` macros, which rustc refuses to resolve by an
//! absolute path from their defining crate. The full scripted transport is
//! `end_to_end.rs`'s; these are the cases small enough to need none.

use uds_services::{
    Address, Ai, DiagnosticSessionType as S, Mtype, NegativeResponseCode as Nrc, Reloads,
    ResponseSink, SResult, ServerParams, SessionTiming, SessionTransition, Sink, TaType,
    Timestamp, TransportEvent, UdsTransport, uds_server,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    VehicleSpeed,
}

impl uds_services::DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        0xF4_0D
    }
    fn from_u16(v: u16) -> Option<Self> {
        (v == 0xF4_0D).then_some(Self::VehicleSpeed)
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), uds_services::RecordError> {
        buf.split_at_checked(1)
            .ok_or(uds_services::RecordError::Short)
    }
}

/// The smallest application: one identifier and session control.
#[derive(Debug)]
struct Ecu;

impl uds_services::ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    // A plain `fn` returning a ready future: an `async fn` with no `.await` is
    // `clippy::unused_async_trait_impl`, which pedantic denies.
    fn read(
        &mut self,
        _did: Did,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), Nrc>> {
        core::future::ready(out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong))
    }
}

impl uds_services::DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _r: bool) {}
}

/// A transport that confirms a transmission the server never made.
/// (Review focus 2.)
#[derive(Debug)]
struct Spurious {
    done: bool,
}

impl UdsTransport for Spurious {
    type Error = ();
    fn t_data_req(
        &mut self,
        _ai: Ai,
        _d: &[u8],
    ) -> impl core::future::Future<Output = Result<(), ()>> {
        core::future::ready(Ok(()))
    }
    fn next_event<'b>(
        &mut self,
        _b: &'b mut [u8],
        _d: Option<Timestamp>,
    ) -> impl core::future::Future<Output = Result<TransportEvent<'b>, ()>> {
        let event = if self.done {
            Err(())
        } else {
            self.done = true;
            Ok(TransportEvent::DataConf {
                ai: Ai {
                    mtype: Mtype::Diag,
                    sa: Address(0x10),
                    ta: Address(0x0E80),
                    ta_type: TaType::Physical,
                },
                result: SResult::Ok,
            })
        };
        core::future::ready(event)
    }
    fn outbound_max(&self) -> Option<usize> {
        None
    }
    fn channel_timing(&self) -> Reloads {
        Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        }
    }
    fn now(&self) -> Timestamp {
        Timestamp(0)
    }
}

uds_server! {
    Ecu: ReadDataByIdentifier, DiagnosticSessionControl;
    transport = Spurious,
    peers = 1,
    server = Srv,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
};

#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
fn block_on<F: core::future::Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("the fixtures never pend"),
    }
}

/// A spurious confirmation is rejected by the session layer and the driver carries
/// on: `step` returns `Ok`, not an error and not a panic.
#[test]
fn a_spurious_confirmation_is_survived() {
    let mut server = Srv::new(Ecu, Spurious { done: false }, PARAMS);
    assert_eq!(block_on(server.step()), Ok(()));
    assert!(server.transport().done);
    assert_eq!(block_on(server.step()), Err(()));
}
