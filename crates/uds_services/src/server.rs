//! The run loop.
//!
//! ``UDSSVC_ARCH_0040`` — this crate owns the loop. It supplies `uds_session` with
//! timestamps and drains its outputs, calls a transport to send and receive bytes, and
//! dispatches inbound requests to the application's handlers. No other crate in the stack
//! contains a driver, and a consuming application writes none.
//!
//! ISO 14229-2 defines a service interface between the session layer and its user and
//! names that user as the ISO 14229-1 layer. A third party sitting between them is a
//! component no standard describes, and it is the component that went a full design cycle
//! with no owner.

use crate::pipeline::settle;
use crate::select::{Either, select2};
use crate::services::{Responded, ServiceSet};
use crate::state::ProtocolState;
use crate::storage::{Buffers, Storage};
use crate::transport::{TransportEvent, UdsTransport};
use crate::{AfterSend, Received, ResponseSink, Unsettled};
use core::future::Future;
use core::pin::Pin;
use uds_protocol::{DiagnosticSessionType, NegativeResponseCode, UdsServiceType};
pub use uds_session::ServerParams;
use uds_session::{
    Address, Ai, Association, Cause, Rejection, SResult, Server as SessionServer,
    ServerOutput, ServerParameter, ServerRx, ServerTx, SessionSelection, Solicitation,
    TaType, Timestamp,
};

/// The UDS server: an application's services, its storage, a session layer and a
/// transport.
///
/// Named `Server` although `uds_session::Server` exists, because a consuming application
/// never imports `uds_session` — only a binding author does, and a binding author can
/// alias. The trait an application implements is [`ServiceSet`], so `Server` means one
/// thing here.
///
/// `PEERS` is a parameter rather than a derived constant because the session layer's type
/// is `uds_session::Server<PEERS>`, and an associated const of a generic cannot be a const
/// generic argument. An application does not write it: [`crate::uds_server`]'s
/// `peers = N` supplies it and the macro emits the alias, so the count appears once.
#[derive(Debug)]
pub struct Server<A: ServiceSet, T: UdsTransport, const PEERS: usize> {
    services: A,
    store: A::Store,
    state: A::State,
    session: SessionServer<PEERS>,
    transport: T,
    /// The address every response is sent from (its `S_SA`, ``UDSS_LLR_0051``).
    own: Address,
    /// The `DiagnosticSessionControl` response awaiting its confirmation, if any.
    pending: Option<Pending>,
    /// The parameters the server was built with, against which each session's `P2` pair is
    /// checked in debug builds.
    params: ServerParams,
    /// Whether [`ServiceSet::start_up`] has run.
    started: bool,
}

impl<A: ServiceSet, T: UdsTransport, const PEERS: usize> Server<A, T, PEERS> {
    /// Assemble a server.
    ///
    /// A `const fn`, so a server can be built in place, in a `static` initialiser, rather
    /// than on the stack: the storage is inline and can be several kilobytes, which a
    /// small-stack target could not hold as a temporary. [`Storage::EMPTY`] being an
    /// associated const is what allows it.
    ///
    /// [`Self::step`] takes `&mut self`, so the `static` has to yield a `&'static mut`
    /// once. `static_cell::ConstStaticCell::new(EcuServer::new(..))` and its `take()` do
    /// that without a stack temporary, as [`crate::uds_server`]'s example shows. A plain
    /// `static SERVER: EcuServer` lands in read-only memory and can never be stepped, and
    /// `StaticCell::init(EcuServer::new(..))` builds the value on the stack before moving
    /// it. In a `ConstStaticCell` the server lives in initialised data, so its initial
    /// image, buffers included, is also stored in flash.
    ///
    /// The driver builds its own session, so an application never names `uds_session`.
    ///
    /// `PEERS` must be 1, checked at compile time: milestone 1 keeps one slot for a
    /// selecting response awaiting its confirmation (see [`crate::uds_server`]).
    ///
    /// # Arguments
    ///
    /// * `services` - the application's assembled [`ServiceSet`], whose handlers answer
    ///   requests
    /// * `transport` - the [`UdsTransport`] the server receives requests from and sends
    ///   responses through
    /// * `own` - the [`Address`] every response is sent from: its `S_SA`, which
    ///   ``UDSS_LLR_0051`` makes the sending entity's. It is not derived from the request,
    ///   because a functionally addressed request's `S_TA` is the functional group
    ///   address, and a response sent from that address would not say which server
    ///   answered.
    /// * `params` - the session layer's [`ServerParams`]: `tS3_Server` and the
    ///   response-pending lead, which the server enforces, and the `P2Server_max` and
    ///   `P2*Server_max` it enforces only where `services` has no
    ///   `DiagnosticSessionControl`; otherwise each session's
    ///   [`DiagnosticSessionControl::timing`](crate::DiagnosticSessionControl::timing)
    ///   replaces them
    pub const fn new(
        services: A,
        transport: T,
        own: Address,
        params: ServerParams,
    ) -> Self {
        const { assert!(PEERS == 1, "milestone 1 supports `PEERS = 1` only") };
        Self {
            services,
            store: <A::Store as Storage>::EMPTY,
            state: <A::State as ProtocolState>::INITIAL,
            session: SessionServer::new([Association::EMPTY; PEERS], params),
            transport,
            own,
            pending: None,
            params,
            started: false,
        }
    }

    /// The application's services, for whatever it needs them for between events.
    pub fn services(&mut self) -> &mut A {
        &mut self.services
    }

    /// The transport, for inspection between events — the counterpart of
    /// [`Self::services`].
    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Run [`ServiceSet::start_up`] the first time it is called, and never again.
    fn start_up(&mut self) {
        if !core::mem::replace(&mut self.started, true) {
            self.services.start_up(&mut self.state);
        }
    }

    /// Handle one transport event.
    ///
    /// **An `Err` is terminal for this server instance** (``UDSSVC_ARCH_0040``, a
    /// milestone-1 limit). A transport error unwinds through the drains with `?`, so a
    /// session-hook decision a drain already recorded — a `tS3_Server` expiry, a
    /// confirmed session — may not have been applied, and the application's state and
    /// the session layer may disagree afterwards. Do not call `step` on this instance
    /// again. Nothing here recovers it in place: the transport and the services cannot be
    /// taken back out of it.
    ///
    /// # Cancel safety
    ///
    /// The returned future is **not** cancel-safe: drop it only between `step` calls.
    /// Dropped at any await other than the transport's `next_event`, it leaves the
    /// instance in the state the terminal-`Err` paragraph above describes — outputs
    /// drained but not applied, a `Pending` slot cleared without its confirmation
    /// applied, a handler dropped with its association still outstanding — and
    /// `uds_services::State` may disagree with the session layer. Tracked in
    /// `luminartech/uds_stack#19` (<https://github.com/luminartech/uds_stack/issues/19>),
    /// together with recovery after an `Err`.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed. A negative response is not an
    /// error: it is a response, written into the sink.
    pub async fn step(&mut self) -> Result<(), T::Error> {
        self.start_up();
        let Buffers {
            in_flight,
            concurrent,
            response,
        } = self.store.split();
        let deadline = self.session.next_deadline();
        let ev = self
            .transport
            .next_event(&mut in_flight[..], deadline)
            .await?;
        // Sampled here, not earlier: `outbound_max` and `now` take `&self`, but a live
        // `next_event` future refuses even shared access, so both have to be read after
        // that await returns. A timestamp accompanies every input handed to uds_session.
        let now = self.transport.now();
        let (ai, received) = match ev {
            TransportEvent::DataInd { ai, data } => (ai, Received::Whole(data)),
            TransportEvent::DataTooLong { ai, data, .. } => (ai, Received::Truncated(data)),
            TransportEvent::DataConf { ai, result } => {
                // UDSS_LLR_0063 — one matching no association is rejected by the session
                // layer; the drain reads the verdict and nothing changes.
                let reaction = self.session.t_data_conf(now, ai, result);
                let d =
                    drain(reaction, &mut self.transport, &mut self.pending, None).await?;
                apply(&mut self.services, &mut self.state, d.deferred);
                return Ok(());
            }
            TransportEvent::Deadline => {
                let reaction = self.session.tick(now);
                let d =
                    drain(reaction, &mut self.transport, &mut self.pending, None).await?;
                apply(&mut self.services, &mut self.state, d.deferred);
                return Ok(());
            }
            // Periodic has no consumer; a Closed outside a handler ends nothing here.
            TransportEvent::Periodic { .. } | TransportEvent::Closed { .. } => {
                return Ok(());
            }
        };

        let outbound_max = self.transport.outbound_max();
        retime(
            &mut self.services,
            &mut self.state,
            &mut self.session,
            &mut self.transport,
            &mut self.pending,
            self.params,
            now,
        )
        .await?;
        let reaction = self.session.t_data_ind(
            now,
            ai,
            received.bytes(),
            SResult::Ok,
            ServerRx::Request { session: None },
        );
        // Dispatched only once indicated: the indication is what starts `tP2_Server`
        // (UDSS_LLR_0113), without which the 0x78 window does not exist.
        let d = drain(reaction, &mut self.transport, &mut self.pending, None).await?;
        apply(&mut self.services, &mut self.state, d.deferred);
        let Some((ai, _)) = d.indication else {
            return Ok(());
        };
        // The byte is kept as received because a negative response echoes it
        // (ISO 14229-1:2020 Table 21, SIDRQ). An empty request is the pipeline's to
        // settle, and settles without pending, so the 0 is never transmitted.
        let sid = received.bytes().first().copied().unwrap_or(0);
        let serving = Serving::new(&self.services, self.own, ai, sid);

        let mut sink = ResponseSink::new(response, outbound_max);
        // Scoped, because `pin!` binds to the enclosing block: `handler` drops before
        // `sink` is settled and read.
        let served = {
            let handler = core::pin::pin!(self.services.dispatch(
                &mut self.state,
                ai,
                received,
                &mut sink,
            ));
            serve::<A, _, _, PEERS>(
                handler,
                &mut self.session,
                &mut self.transport,
                &mut self.pending,
                &mut concurrent[..],
                serving,
            )
            .await?
        };
        // Recorded mid-handler, applied now that `&mut services`/`&mut state` are free.
        apply(&mut self.services, &mut self.state, served.deferred);
        let serving = serving.leaving_if(&self.services, &served);
        let deferred = conclude::<A, _, PEERS>(
            &mut self.session,
            &mut self.transport,
            &mut self.pending,
            &mut concurrent[..],
            served,
            serving,
            &mut sink,
        )
        .await?;
        apply(&mut self.services, &mut self.state, deferred);
        Ok(())
    }

    /// Run until the transport fails.
    ///
    /// **The `Err` this returns is terminal for this server instance**
    /// (``UDSSVC_ARCH_0040``, a milestone-1 limit), as an `Err` from [`Self::step`] is: a
    /// session-hook decision already recorded may not have been applied, so the
    /// application's state and the session layer may disagree afterwards. Do not call
    /// `run` or `step` on this instance again. Nothing here recovers it in place: the
    /// transport and the services cannot be taken back out of it.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe, for the reason [`Self::step`] is not: dropping this future drops
    /// a `step` future, and not necessarily at the transport's `next_event`. See
    /// `step`'s `# Cancel safety` section, which also names the tracking issue,
    /// `luminartech/uds_stack#19`. To stop between events, drive `step` in your own loop.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returned.
    pub async fn run(&mut self) -> Result<core::convert::Infallible, T::Error> {
        loop {
            self.step().await?;
        }
    }
}

/// Load the `P2` pair of the session in force into the session layer, so the request about
/// to be indicated at `now` opens the window its session advertised.
///
/// The session layer is ticked at `now` first and its expiries applied, so a `tS3_Server`
/// running out at that instant returns to the default session before its pair is read.
/// The parameters are then set at the same `now`, which can expire nothing further, and
/// ``UDSS_LLR_0076`` leaves any window already open on the value it was loaded with.
///
/// A debug build panics where the pair leaves `params`' response-pending lead ill formed:
/// the enhanced overrun would then come due before ``UDSS_LLR_0119`` admits its 0x78.
async fn retime<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    services: &mut A,
    state: &mut A::State,
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    params: ServerParams,
    now: Timestamp,
) -> Result<(), T::Error> {
    let tick = drain(session.tick(now), transport, pending, None).await?;
    apply(services, state, tick.deferred);
    let Some(timing) = services.session_timing(state) else {
        return Ok(());
    };
    debug_assert!(
        ServerParams {
            p2_server_max: timing.p2_server_max(),
            p2_star_server_max: timing.p2_star_server_max(),
            ..params
        }
        .is_well_formed(),
        "DiagnosticSessionControl::timing returned a pair the response-pending lead does \
         not fit (ServerParams::is_well_formed)",
    );
    for parameter in [
        ServerParameter::P2ServerMax(timing.p2_server_max()),
        ServerParameter::P2StarServerMax(timing.p2_star_server_max()),
    ] {
        let d = drain(
            session.set_parameter(now, parameter),
            transport,
            pending,
            None,
        )
        .await?;
        apply(services, state, d.deferred);
    }
    Ok(())
}

/// The `DiagnosticSessionControl` response in flight: its session takes effect on the
/// `Confirm` that reports it sent (``UDSS_LLR_0085``). One slot: a server
/// answers one request at a time (``UDSS_LLR_0108``), so with one peer one selecting
/// response is in flight at most.
///
/// **Milestone 1 accepts `peers = 1` only** ([`Server::new`] fails to compile for any
/// other `PEERS`). A server answers one request at a time, but a response awaiting its
/// confirmation is no longer the request in progress: with a second tester, that
/// tester's selecting response could be accepted while the first is still unconfirmed,
/// and it would overwrite this slot. The first confirmation would then match nothing and
/// its session would never be applied.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// The response's addressing, exactly as submitted: `t_data_conf` matches by it.
    ai: Ai,
    /// The session the response selected.
    selected: DiagnosticSessionType,
}

/// Where a response to `request` goes: the request went tester -> ECU, the response goes
/// back the other way. The exact `Ai` `s_data_req` registers, and so the one the
/// response's `DataConf` must carry to match it.
///
/// Sent from `own`, never from the request's `S_TA`: for a functional request that is
/// the group address, and ``UDSS_LLR_0051`` makes `S_SA` the sending entity's address.
///
/// Physically addressed whatever the request was: a server answers the one client that
/// asked, so a response to a functional request is a physical message to that client.
/// That is an observation the requirement set relies on, not a clause of
/// ISO 14229-2:2021 9.6 (``UDSS_LLR_0026``'s rationale).
const fn reply_address(own: Address, request: Ai) -> Ai {
    Ai {
        sa: own,
        ta: request.sa,
        ta_type: TaType::Physical,
        ..request
    }
}

/// ``UDSS_LLR_0073`` — the session layer learns only default/non-default.
fn selection_of(session: DiagnosticSessionType) -> SessionSelection {
    if matches!(session, DiagnosticSessionType::DefaultSession) {
        SessionSelection::Default
    } else {
        SessionSelection::NonDefault
    }
}

/// What a drain found that the application must hear about, applied by [`apply`] where
/// `&mut services`/`&mut state` are free.
#[derive(Debug, Clone, Copy)]
struct Deferred {
    /// `tS3_Server` expired (``UDSS_LLR_0100``).
    timed_out: bool,
    /// A selecting response was confirmed sent, or its suppression completed.
    confirmed: Option<DiagnosticSessionType>,
}

impl Deferred {
    const NONE: Self = Self {
        timed_out: false,
        confirmed: None,
    };

    /// Both records, in arrival order: `self` was recorded first.
    ///
    /// A later timeout clears an earlier confirmation. [`apply`] runs the timeout before
    /// the confirmation, so keeping both would leave the application in the confirmed
    /// session while the session layer, which expired it afterwards, is in the default
    /// one. A later confirmation after an earlier timeout keeps both, which is already
    /// the order `apply` runs them in; of two confirmations the later wins.
    fn merge(self, later: Self) -> Self {
        Self {
            timed_out: self.timed_out || later.timed_out,
            confirmed: if later.timed_out {
                later.confirmed
            } else {
                later.confirmed.or(self.confirmed)
            },
        }
    }
}

/// What one drain found, beyond what it already acted on.
#[derive(Debug)]
struct Drained<'d> {
    /// A request received successfully.
    indication: Option<(Ai, &'d [u8])>,
    /// ``UDSS_LLR_0117`` — `tP2_Server` expired with no response transmitted.
    overran: bool,
    /// The reaction's verdict, where the input was refused.
    rejected: Option<Rejection>,
    /// What the application must hear about.
    deferred: Deferred,
}

/// What follows a transmission: [`AfterSend::ServerLeaves`] for the final positive
/// `DiagnosticSessionControl` response to `leaving`, and [`AfterSend::Continue`] for
/// everything else — a 0x78 to the same addressing and a negative response included, as
/// neither starts with the positive response identifier.
fn after_send(leaving: Option<Ai>, ai: Ai, data: &[u8]) -> AfterSend {
    let positive = UdsServiceType::DiagnosticSessionControl.to_response_sid();
    if leaving == Some(ai) && data.first() == Some(&positive) {
        AfterSend::ServerLeaves
    } else {
        AfterSend::Continue
    }
}

/// The one place every session output is handled (``UDSSVC_ARCH_0040``). Every reaction,
/// from every input, passes through here, because an expiry can surface from any of
/// them. Acts on what needs only the transport and the pending record; records the rest.
///
/// Inside the drain, not after it: a reaction may yield several `Transmit`s and all of
/// them must reach the transport. The reaction borrows the session and `t_data_req`
/// borrows the transport, which is why they arrive as separate arguments rather than as
/// one `&mut self` — disjoint fields are disjoint borrows only while nothing has merged
/// them.
async fn drain<'d, T: UdsTransport, const PEERS: usize>(
    mut reaction: uds_session::ServerReaction<'_, 'd, PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    leaving: Option<Ai>,
) -> Result<Drained<'d>, T::Error> {
    let mut found = Drained {
        indication: None,
        overran: false,
        rejected: None,
        deferred: Deferred::NONE,
    };
    for out in reaction.outputs() {
        match out {
            // milestone-1 limit: an `Err` here drops what this drain recorded; see
            // `Server::step`'s doc.
            ServerOutput::Transmit { ai, data } => {
                transport
                    .t_data_req(ai, data, after_send(leaving, ai, data))
                    .await?;
            }
            ServerOutput::Indicate {
                ai,
                data,
                result: SResult::Ok,
            } => found.indication = Some((ai, data)),
            ServerOutput::Confirm { ai, result } => {
                // Only the selecting response's own confirmation settles the slot; a
                // failed one clears it and leaves the session where it was
                // (``UDSS_LLR_0085`` acts on a successful transmission only).
                if let Some(p) = *pending
                    && p.ai == ai
                {
                    *pending = None;
                    if result == SResult::Ok {
                        found.deferred.confirmed = Some(p.selected);
                    }
                }
            }
            ServerOutput::SessionTimeout { .. } => found.deferred.timed_out = true,
            ServerOutput::ResponseOverrun { .. } => found.overran = true,
            // A failed reception's `Indicate`, whose data means nothing (UDSS_LLR_0035),
            // and whatever else UDSS_LLR_0012's open enumeration adds.
            _ => {}
        }
    }
    let uds_session::Finished {
        outcome,
        rest: _drained,
    } = reaction.finish();
    found.rejected = outcome.err();
    Ok(found)
}

/// Act on what a drain recorded: the timeout, then the confirmation. The only caller of
/// the session hooks.
///
/// That fixed order is the arrival order. Within one drain, ``UDSS_LLR_0081`` puts the
/// expiry snapshots ahead of the input's own outputs, so a `SessionTimeout` always
/// precedes the `Confirm` beside it. Across drains, [`Deferred::merge`] drops a
/// confirmation that a later timeout overtook, so whatever survives to here is a
/// timeout followed by a confirmation, or only one of them.
fn apply<A: ServiceSet>(services: &mut A, state: &mut A::State, d: Deferred) {
    if d.timed_out {
        services.session_timed_out(state);
    }
    if let Some(selected) = d.confirmed {
        services.session_confirmed(state, selected);
    }
}

/// The request in progress, as its responses and its completion report name it.
#[derive(Debug, Clone, Copy)]
struct Serving {
    /// The request's addressing, which its completion report names.
    ai: Ai,
    /// The address every response is sent from, a busy refusal's included.
    own: Address,
    /// The request's addressing, swapped: where its responses go.
    reply_to: Ai,
    /// The request's service identifier, which a negative response echoes.
    sid: u8,
    /// ``UDSSVC_ARCH_0032`` — whether the service admits a 0x78 at all, as
    /// `ServiceSet::may_respond_pending` reported it.
    may_pend: bool,
    /// `reply_to`, where the final positive response selects a session that
    /// `ServiceSet::leaves_running_software`: the one message sent
    /// [`AfterSend::ServerLeaves`]. Known once the handler has finished.
    leaving: Option<Ai>,
}

impl Serving {
    /// The request `ai` sent, its service identifier `sid`, before it is dispatched.
    ///
    /// ``UDSSVC_ARCH_0032`` — whether 0x78 is admissible is the service's to say, never
    /// the driver's. Resolved before dispatch, which holds `&mut services` until it
    /// completes.
    fn new<A: ServiceSet>(services: &A, own: Address, ai: Ai, sid: u8) -> Self {
        Self {
            ai,
            own,
            reply_to: reply_address(own, ai),
            sid,
            may_pend: services.may_respond_pending(UdsServiceType::from_request_sid(sid)),
            leaving: None,
        }
    }

    /// The same, with [`Self::leaving`] set where `served` selects a session that
    /// `services` leave the running software to enter.
    fn leaving_if<A: ServiceSet>(self, services: &A, served: &Served) -> Self {
        Self {
            leaving: served
                .selected()
                .filter(|&selected| services.leaves_running_software(selected))
                .map(|_| self.reply_to),
            ..self
        }
    }
}

/// How the request's handling ended.
#[derive(Debug, Clone, Copy)]
enum Ended {
    /// It ran to completion, with this outcome for `settle`.
    Finished(Unsettled),
    /// The link closed first: the handler was dropped and no response can be sent.
    Closed,
}

impl Served {
    /// The session the handler's outcome selects, where it finished and selects one.
    fn selected(&self) -> Option<DiagnosticSessionType> {
        match self.ended {
            Ended::Finished(unsettled) => unsettled
                .parts()
                .and_then(|(_, outcome)| outcome.ok().flatten()),
            Ended::Closed => None,
        }
    }
}

/// What [`serve`] hands back once the handler has finished, for the caller to settle.
#[derive(Debug, Clone, Copy)]
struct Served {
    /// How the handling ended.
    ended: Ended,
    /// ``UDSSVC_ARCH_0009`` rule 3's input: a 0x78 for this request was accepted for
    /// transmission while the handler ran.
    pending_sent: bool,
    /// What the drains made meanwhile recorded for the application.
    deferred: Deferred,
}

/// Poll the handler to completion, answering what the transport delivers meanwhile.
///
/// Returns how the handling ended, whether a 0x78 was accepted meanwhile, and what the
/// drains recorded for the application, which waits until the handler releases
/// `&mut services`/`&mut state`. The caller settles: the handler future holds the sink
/// until it drops. `waiting` is scoped per iteration so it drops before the 0x78 path
/// needs `&mut transport`.
///
/// A message arriving meanwhile finds the protocol instance occupied (ISO 14229-1:2020
/// 8.7.6): the keep-alive `TesterPresent` bypasses it ([`keep_alive`]) and anything else
/// is refused ([`refuse_busy`]). Every event other than a close first answers an overrun
/// due by its own timestamp ([`overrun_at`]), so none is lost to the event's own input.
async fn serve<
    A: ServiceSet,
    H: Future<Output = Unsettled>,
    T: UdsTransport,
    const PEERS: usize,
>(
    mut handler: Pin<&mut H>,
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    concurrent: &mut [u8],
    serving: Serving,
) -> Result<Served, T::Error> {
    let mut deferred = Deferred::NONE;
    let mut pending_sent = false;
    let mut owed = false;
    // A loop over `handler.as_mut()`, not a one-shot: a plain
    // `select2(handler, waiting)` drops the handler the moment the deadline wins,
    // which is the opposite of what a response-pending is for.
    loop {
        let deadline = session.next_deadline();
        let event = {
            let waiting = core::pin::pin!(transport.next_event(concurrent, deadline));
            select2(handler.as_mut(), waiting).await
        };
        // milestone-1 limit: an `Err` at any `?` below drops what was recorded; see
        // `Server::step`'s doc.
        match event {
            Either::Left(unsettled) => {
                return Ok(Served {
                    ended: Ended::Finished(unsettled),
                    pending_sent,
                    deferred,
                });
            }
            Either::Right(Err(e)) => return Err(e),
            Either::Right(Ok(TransportEvent::Closed { .. })) => {
                // The exchange is over either way: a server does not reconnect
                // (ISO 13400-2 REQ 8.DoIP-144 puts routing activation on the
                // client). A close reaching *this* arm is never REQ 7.9's or
                // 7.11's — those follow a positive response, and nothing positive
                // has been sent yet — so whether it was expected changes nothing.
                return Ok(Served {
                    ended: Ended::Closed,
                    pending_sent,
                    deferred,
                });
            }
            Either::Right(Ok(event)) => {
                let now = transport.now();
                let o = overrun_at(session, transport, pending, now, serving).await?;
                // UDSSVC_ARCH_0009 rule 3 — kept for `settle`, which runs once the
                // handler has finished. Never cleared: one accepted 0x78 is enough.
                pending_sent |= o.sent_pending;
                owed |= o.owed;
                deferred = deferred.merge(o.deferred);
                match event {
                    TransportEvent::DataConf { ai, result } => {
                        // The 0x78's confirmation, or any other: it frees its association
                        // and opens the enhanced window (UDSS_LLR_0116).
                        let reaction = session.t_data_conf(now, ai, result);
                        let d = drain(reaction, transport, pending, None).await?;
                        deferred = deferred.merge(d.deferred);
                        if owed {
                            // UDSS_LLR_0061/0062 refused the 0x78 for want of the
                            // association this confirmation may have freed, and the
                            // overrun is reported once.
                            let o =
                                submit_pending(session, transport, pending, now, serving);
                            let o = o.await?;
                            pending_sent |= o.sent_pending;
                            owed = o.owed;
                            deferred = deferred.merge(o.deferred);
                        }
                    }
                    TransportEvent::DataInd { ai, data }
                        if A::is_concurrent_exception(data, ai) =>
                    {
                        let d = keep_alive(session, transport, pending, now, ai, data);
                        deferred = deferred.merge(d.await?);
                    }
                    TransportEvent::DataInd { ai, data }
                    | TransportEvent::DataTooLong { ai, data, .. } => {
                        let own = serving.own;
                        let d =
                            refuse_busy(session, transport, pending, now, own, ai, data);
                        deferred = deferred.merge(d.await?);
                    }
                    // A deadline is answered by `overrun_at` alone; Periodic has no
                    // consumer; Closed returned above.
                    TransportEvent::Deadline
                    | TransportEvent::Periodic { .. }
                    | TransportEvent::Closed { .. } => {}
                }
            }
        }
    }
}

/// ISO 14229-1:2020 8.7.6's first exception: the keep-alive `TesterPresent` bypasses the
/// service in progress. Indicated as keep-alive, never as a request, so it replaces no
/// service (``UDSS_LLR_0108``) and reloads `tS3_Server` only where that is running and it
/// is from the controlling client (``UDSS_LLR_0095``, ``UDSS_LLR_0096``); neither
/// dispatched nor answered, its positive response being suppressed.
async fn keep_alive<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    now: Timestamp,
    ai: Ai,
    data: &[u8],
) -> Result<Deferred, T::Error> {
    let reaction = session.t_data_ind(now, ai, data, SResult::Ok, ServerRx::KeepAlive);
    Ok(drain(reaction, transport, pending, None).await?.deferred)
}

/// ISO 14229-1:2020 8.7.6 — any other message arriving while a service is in progress
/// finds the protocol instance occupied and is refused `busyRepeatRequest` (Annex A) from
/// its service identifier alone; the service in progress continues.
///
/// Never indicated as a request, which ``UDSS_LLR_0108`` would have replace the service
/// in progress, and submitted as a busy refusal, which answers no service
/// (``UDSS_LLR_0187``). A refusal the session layer refuses in turn, the client's
/// addressing still awaiting a confirmation, is dropped: ignoring the request is Annex J
/// Figure J.2's other branch, and the client's own timeout recovers it. A message with no
/// service identifier has nothing to echo and is dropped too (``UDSSVC_ARCH_0005``).
async fn refuse_busy<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    now: Timestamp,
    own: Address,
    ai: Ai,
    data: &[u8],
) -> Result<Deferred, T::Error> {
    let Some(&sid) = data.first() else {
        return Ok(Deferred::NONE);
    };
    let busy = [0x7F, sid, u8::from(NegativeResponseCode::BusyRepeatRequest)];
    let reply_to = reply_address(own, ai);
    let reaction = session.s_data_req(now, reply_to, &busy, ServerTx::BusyRepeatRequest);
    Ok(drain(reaction, transport, pending, None).await?.deferred)
}

/// End the request once its handling has: settle and answer it, or, where the link closed
/// first, report it over with nothing selected (``UDSS_LLR_0074``), no response being
/// sendable and none having been decided against.
async fn conclude<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    concurrent: &mut [u8],
    served: Served,
    serving: Serving,
    sink: &mut ResponseSink<'_>,
) -> Result<Deferred, T::Error> {
    match served.ended {
        Ended::Finished(unsettled) => {
            // UDSSVC_ARCH_0016 — the driver settles: rule 3's input is final only now
            // that the handler has finished, and only this loop knows it.
            let outcome = settle(serving.ai, unsettled, served.pending_sent, sink);
            let response = sink.written_bytes();
            answer::<A, _, PEERS>(
                session, transport, pending, concurrent, outcome, serving, response,
            )
            .await
        }
        Ended::Closed => complete(session, transport, pending, serving.ai, None).await,
    }
}

/// Submit the handler's outcome: a final response, or the completion report a
/// suppressed request owes.
///
/// A free function rather than a method because `response` borrows the server's store.
/// Returns what the application must hear about, for the caller to [`apply`].
///
/// A final response refused under ``UDSS_LLR_0061`` or ``UDSS_LLR_0062`` is waited out
/// and resubmitted once. The first is reachable because [`select2`] lets the handler win
/// a tie, so a handler can finish after a 0x78 was accepted and before that 0x78's
/// `DataConf` was drained; the second wherever every association is still awaiting a
/// confirmation. A refusal for any other cause loses the response. The tester has had
/// at most a 0x78, and the service stays in progress until its timer runs out. That is
/// a milestone-1 limit; nothing retries it.
async fn answer<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    concurrent: &mut [u8],
    outcome: Responded,
    serving: Serving,
    response: &[u8],
) -> Result<Deferred, T::Error> {
    let selected = match outcome {
        Responded::Yes { session: selected } => selected,
        Responded::Suppressed { session: selected } => {
            return complete(session, transport, pending, serving.ai, selected).await;
        }
    };
    let now = transport.now();
    let reaction = final_response(session, now, serving.reply_to, response, selected);
    // milestone-1 limit: an `Err` at any `?` below drops what was recorded; see
    // `Server::step`'s doc.
    let mut d = drain(reaction, transport, pending, serving.leaving).await?;
    let mut deferred = d.deferred;
    let awaited = match d.rejected {
        // UDSS_LLR_0061 — only `reply_to`'s own confirmation frees its addressing.
        Some(r) if r.contains(Cause::AssociationOutstanding) => Some(Awaited::Reply),
        // UDSS_LLR_0062 — any confirmation frees an association.
        Some(r) if r.contains(Cause::NoAssociationFree) => Some(Awaited::Any),
        _ => None,
    };
    if let Some(awaited) = awaited {
        let waited = await_confirmation::<A, _, PEERS>(
            session, transport, pending, concurrent, serving, awaited,
        );
        let Some(waited) = waited.await? else {
            // The link closed first, as in `serve`'s `Ended::Closed`.
            let closed = complete(session, transport, pending, serving.ai, None).await?;
            return Ok(deferred.merge(closed));
        };
        deferred = deferred.merge(waited);
        let now = transport.now();
        let reaction = final_response(session, now, serving.reply_to, response, selected);
        d = drain(reaction, transport, pending, serving.leaving).await?;
        deferred = deferred.merge(d.deferred);
    }
    match d.rejected {
        // Recorded only once the submission is accepted, so a refused response never
        // leaves a pending selection behind, and never disturbs one already in
        // flight. `reply_to` is the association `t_data_conf` will match.
        None => {
            if let Some(selected) = selected {
                *pending = Some(Pending {
                    ai: serving.reply_to,
                    selected,
                });
            }
        }
        // UDSSVC_ARCH_0040 — a milestone-1 limit: a final response refused for any
        // cause but an outstanding or unavailable association (or refused again after
        // waiting one out) is lost, with no retry. See `answer`'s doc.
        Some(_refused) => {}
    }
    Ok(deferred)
}

/// The final response's submission, built in one place because [`answer`] may make it
/// twice.
fn final_response<'s, 'd, const PEERS: usize>(
    session: &'s mut SessionServer<PEERS>,
    now: Timestamp,
    reply_to: Ai,
    response: &'d [u8],
    selected: Option<DiagnosticSessionType>,
) -> uds_session::ServerReaction<'s, 'd, PEERS> {
    session.s_data_req(
        now,
        reply_to,
        response,
        ServerTx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: selected.map(selection_of),
        },
    )
}

/// ``UDSS_LLR_0074`` — report a request complete with no response, and with it the
/// selection it made.
///
/// ``UDSS_LLR_0086`` / ``0098`` — the completion report is the moment, so the selection
/// follows whatever the report's own drain recorded.
async fn complete<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    ai: Ai,
    selected: Option<DiagnosticSessionType>,
) -> Result<Deferred, T::Error> {
    let now = transport.now();
    let class = ServerRx::Request {
        session: selected.map(selection_of),
    };
    let reaction = session.completion_report(now, ai, class);
    let d = drain(reaction, transport, pending, None).await?;
    Ok(d.deferred.merge(Deferred {
        timed_out: false,
        confirmed: selected,
    }))
}

/// Which confirmation [`await_confirmation`] waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Awaited {
    /// ``UDSS_LLR_0061`` — the one for `serving.reply_to`, the addressing refused.
    Reply,
    /// ``UDSS_LLR_0062`` — any accepted one, since any frees an association.
    Any,
}

/// Wait for a transmission outstanding to be confirmed, so the final response
/// ``UDSS_LLR_0061`` or ``UDSS_LLR_0062`` refused can be resubmitted.
///
/// Each event takes the path it takes mid-handler: an overrun due by its timestamp is
/// answered first ([`overrun_at`]), a `DataConf` then reaches `t_data_conf`, and a
/// message arriving is bypassed or refused as in [`serve`]. A 0x78 refused here is not
/// owed: the final response the awaited confirmation frees the way for supersedes it.
///
/// Returns what the drains recorded once the awaited `DataConf` has been drained and
/// accepted — `serving.reply_to`'s for [`Awaited::Reply`], any for [`Awaited::Any`] — or
/// `None` where the link closed first.
///
/// **Unbounded** (milestone-1 limit, ``UDSSVC_ARCH_0040``). The wait ends on the awaited
/// `DataConf`, a `Closed`, or a transport error, and on nothing else: a transport that
/// never confirms stalls the driver here. That is the assumption of use
/// [`UdsTransport`] states, a `DataConf` for every accepted `t_data_req`.
async fn await_confirmation<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    concurrent: &mut [u8],
    serving: Serving,
    awaited: Awaited,
) -> Result<Option<Deferred>, T::Error> {
    let mut deferred = Deferred::NONE;
    loop {
        let deadline = session.next_deadline();
        let event = transport.next_event(concurrent, deadline).await?;
        if let TransportEvent::Closed { .. } = event {
            return Ok(None);
        }
        // The handler's outcome is already settled, so whether a 0x78 goes out no
        // longer reaches `settle`; only what the drains recorded is kept.
        let now = transport.now();
        let o = overrun_at(session, transport, pending, now, serving).await?;
        deferred = deferred.merge(o.deferred);
        match event {
            TransportEvent::DataConf { ai, result } => {
                let reaction = session.t_data_conf(now, ai, result);
                let d = drain(reaction, transport, pending, None).await?;
                deferred = deferred.merge(d.deferred);
                let matches = awaited == Awaited::Any || ai == serving.reply_to;
                if matches && d.rejected.is_none() {
                    return Ok(Some(deferred));
                }
            }
            TransportEvent::DataInd { ai, data }
                if A::is_concurrent_exception(data, ai) =>
            {
                let d = keep_alive(session, transport, pending, now, ai, data).await?;
                deferred = deferred.merge(d);
            }
            TransportEvent::DataInd { ai, data }
            | TransportEvent::DataTooLong { ai, data, .. } => {
                let own = serving.own;
                let d = refuse_busy(session, transport, pending, now, own, ai, data);
                deferred = deferred.merge(d.await?);
            }
            TransportEvent::Deadline
            | TransportEvent::Periodic { .. }
            | TransportEvent::Closed { .. } => {}
        }
    }
}

/// What `answer_overrun` or `submit_pending` learned: what became of the 0x78, and what
/// the drains recorded.
#[derive(Debug, Clone, Copy)]
struct Overrun {
    /// The 0x78 was accepted by `s_data_req` (``UDSSVC_ARCH_0009`` rule 3).
    sent_pending: bool,
    /// The 0x78 was refused for want of an association (``UDSS_LLR_0061``,
    /// ``UDSS_LLR_0062``), so a confirmation freeing one makes it submittable again.
    owed: bool,
    /// What the tick's and the submission's drains recorded.
    deferred: Deferred,
}

/// Answer an overrun due by `now` before anything else arriving at `now` reaches the
/// session layer.
///
/// Every session-layer input expires its timers first, and ``UDSS_LLR_0117`` reports an
/// overrun once and stops `tP2_Server`, so an overrun first seen in another input's
/// drain would go unanswered. Ticking here, at the timestamp that input will carry,
/// leaves it nothing to expire. Where the service admits no 0x78 (``UDSSVC_ARCH_0032``),
/// the tick is only drained.
async fn overrun_at<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    now: Timestamp,
    serving: Serving,
) -> Result<Overrun, T::Error> {
    if serving.may_pend {
        return answer_overrun(session, transport, pending, now, serving).await;
    }
    let d = drain(session.tick(now), transport, pending, None).await?;
    Ok(Overrun {
        sent_pending: false,
        owed: false,
        deferred: d.deferred,
    })
}

/// Answer `requestCorrectlyReceivedResponsePending` (0x78) if one has come due.
///
/// ``UDSSVC_ARCH_0031``/``0032`` — the deadline passing is not itself the overrun: the
/// session layer is ticked and ``UDSS_LLR_0117``'s `ResponseOverrun` is the output that
/// says a 0x78 is owed. It is an ordinary transmission, and whether it was *accepted* is
/// the verdict ``UDSSVC_ARCH_0009`` rule 3 reads.
async fn answer_overrun<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    now: Timestamp,
    serving: Serving,
) -> Result<Overrun, T::Error> {
    let tick = session.tick(now);
    let first = drain(tick, transport, pending, None).await?;
    if !first.overran {
        return Ok(Overrun {
            sent_pending: false,
            owed: false,
            deferred: first.deferred,
        });
    }
    let second = submit_pending(session, transport, pending, now, serving).await?;
    Ok(Overrun {
        deferred: first.deferred.merge(second.deferred),
        ..second
    })
}

/// Submit the 0x78 for `serving`.
///
/// The overrun that makes it due is reported once, so a 0x78 refused because the
/// client's addressing awaits a confirmation — a busy refusal's, or a final response's
/// to another request — is reported `owed`, for the caller to resubmit once a
/// confirmation has freed it.
async fn submit_pending<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    now: Timestamp,
    serving: Serving,
) -> Result<Overrun, T::Error> {
    let bytes = [0x7F_u8, serving.sid, 0x78];
    let reaction =
        session.s_data_req(now, serving.reply_to, &bytes, ServerTx::ResponsePending);
    let d = drain(reaction, transport, pending, None).await?;
    Ok(Overrun {
        sent_pending: d.rejected.is_none(),
        owed: d.rejected.is_some_and(|r| {
            r.contains(Cause::AssociationOutstanding)
                || r.contains(Cause::NoAssociationFree)
        }),
        deferred: d.deferred,
    })
}

#[cfg(test)]
mod tests {
    use super::{Deferred, after_send};
    use crate::AfterSend;
    use uds_protocol::DiagnosticSessionType as S;
    use uds_session::{Address, Ai, Mtype, TaType};

    const TO_TESTER: Ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0E00),
        ta: Address(0x0E80),
        ta_type: TaType::Physical,
    };

    /// W4 addendum item 3 — only the positive `DiagnosticSessionControl` response to the
    /// leaving request's addressing is `ServerLeaves`: never the 0x78 sharing that
    /// addressing, a negative response, another tester's response, or anything sent
    /// while no session change leaves.
    #[test]
    fn only_the_leaving_positive_response_is_flagged() {
        let other = Ai {
            ta: Address(0x0E81),
            ..TO_TESTER
        };
        let leaving = Some(TO_TESTER);
        let positive = [0x50, 0x02, 0x00, 0x32, 0x01, 0xF4];
        assert_eq!(
            after_send(leaving, TO_TESTER, &positive),
            AfterSend::ServerLeaves
        );
        for (ai, data) in [
            (TO_TESTER, &[0x7F, 0x10, 0x78][..]),
            (TO_TESTER, &[0x7F, 0x10, 0x22][..]),
            (TO_TESTER, &[0x51, 0x01][..]),
            (other, &positive[..]),
        ] {
            assert_eq!(
                after_send(leaving, ai, data),
                AfterSend::Continue,
                "{data:02X?}"
            );
        }
        assert_eq!(after_send(None, TO_TESTER, &positive), AfterSend::Continue);
    }

    const TIMEOUT: Deferred = Deferred {
        timed_out: true,
        confirmed: None,
    };

    const fn confirm(session: S) -> Deferred {
        Deferred {
            timed_out: false,
            confirmed: Some(session),
        }
    }

    /// A confirmation recorded mid-handler and then overtaken by a `tS3_Server` expiry
    /// is dropped. `apply` runs the timeout first, so keeping it would leave the
    /// application in the confirmed session while the session layer is in the default
    /// one.
    #[test]
    fn a_later_timeout_clears_an_earlier_confirmation() {
        let merged = confirm(S::ExtendedDiagnosticSession).merge(TIMEOUT);
        assert!(merged.timed_out);
        assert_eq!(merged.confirmed, None);
    }

    /// A confirmation after a timeout keeps both, and `apply`'s fixed order is then
    /// their arrival order.
    #[test]
    fn a_later_confirmation_after_a_timeout_keeps_both() {
        let merged = TIMEOUT.merge(confirm(S::ExtendedDiagnosticSession));
        assert!(merged.timed_out);
        assert_eq!(merged.confirmed, Some(S::ExtendedDiagnosticSession));
    }

    /// Of two confirmations, the later is the session in force.
    #[test]
    fn of_two_confirmations_the_later_wins() {
        let merged =
            confirm(S::ExtendedDiagnosticSession).merge(confirm(S::DefaultSession));
        assert!(!merged.timed_out);
        assert_eq!(merged.confirmed, Some(S::DefaultSession));
    }
}
