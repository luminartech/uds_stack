//! The run loop.
//!
//! ``UDSSVC_ARCH_0040``: this crate owns the loop. It supplies `uds_session` with
//! timestamps and drains its outputs, calls a transport to send and receive bytes, and
//! dispatches inbound requests to the application's handlers. No other crate in the stack
//! contains a driver, and a consuming application writes none: ISO 14229-2 names the
//! session layer's user as the ISO 14229-1 layer, which is this crate.

use crate::pipeline::settle;
use crate::select::{Either, select2};
use crate::services::session::selection_of;
use crate::services::{AfterReset, Effect, Responded, ServiceSet};
use crate::state::ProtocolState;
use crate::storage::{Buffers, Storage};
use crate::transport::{TransportEvent, UdsTransport};
use crate::{AfterSend, Received, ResponseSink, Unsettled};
use core::future::Future;
use core::pin::Pin;
use uds_protocol::{NegativeResponseCode, ResetType, UdsServiceType};
pub use uds_session::ServerParams;
use uds_session::{
    Address, Ai, Association, Cause, Rejection, SResult, Server as SessionServer,
    ServerOutput, ServerParameter, ServerRx, ServerTx, Solicitation, TaType, Timestamp,
};

/// The UDS server: an application's services, its storage, a session layer and a
/// transport.
///
/// `A` is the [`ServiceSet`] marker [`crate::uds_server`] generates; the application it
/// holds is `A::App`. `PEERS` sizes the session layer's `uds_session::Server<PEERS>`;
/// [`crate::uds_server`]'s `peers = N` supplies it through the alias it emits, so an
/// application never writes it.
#[derive(Debug)]
pub struct Server<A: ServiceSet, T: UdsTransport, const PEERS: usize> {
    services: A::App,
    store: A::Store,
    state: A::State,
    session: SessionServer<PEERS>,
    transport: T,
    /// The address every response is sent from (its `S_SA`, ``UDSS_LLR_0051``).
    own: Address,
    /// The response awaiting its confirmation, if any, a `DiagnosticSessionControl`
    /// response or an `ECUReset` one, and the change of one whose confirmation failed.
    pending: Unconfirmed,
    /// The change a handler accepted, a session it prepared or a reset, from the
    /// handler's end until its response is submitted, its completion reported or the
    /// change abandoned.
    unmade: Option<Effect>,
    /// Whether the step in progress, if any, awaits the transport's next event between
    /// requests, the one await it is dropped at, or fails at, with the server serving
    /// on.
    idle: bool,
    /// The parameters the server was built with, against which each session's `P2` pair is
    /// checked in debug builds.
    params: ServerParams,
    /// Whether [`ServiceSet::start_up`] has run since the server was built or last stopped.
    started: bool,
}

impl<A: ServiceSet, T: UdsTransport, const PEERS: usize> Server<A, T, PEERS> {
    /// Assemble a server.
    ///
    /// A `const fn`, so a server whose inline storage runs to kilobytes is built in a
    /// `static` initializer rather than as a stack temporary. [`Self::step`] takes
    /// `&mut self`, so the `static` is a `static_cell::ConstStaticCell` whose `take()`
    /// yields the one `&'static mut`, as [`crate::uds_server`]'s example shows; a plain
    /// `static` is read-only and cannot be stepped. What the transport learns only at run
    /// time, such as the identity `uds_on_ip`'s `DoIpTransport` announces, is set through
    /// [`Self::transport_mut`] before the first step. A transport with no `const`
    /// constructor puts its server in a `StaticCell` through
    /// `init_with(|| EcuServer::new(..))`, built on the caller's stack and copied in.
    ///
    /// The driver builds its own session, so an application never names `uds_session`.
    ///
    /// `PEERS` must be 1, checked at compile time (see [`crate::uds_server`]).
    ///
    /// # Arguments
    ///
    /// * `services` - the application, whose handlers answer requests: the
    ///   [`ServiceSet::App`] of `A`
    /// * `transport` - the [`UdsTransport`] the server receives requests from and sends
    ///   responses through
    /// * `own` - the [`Address`] every response is sent from: its `S_SA`, which
    ///   ``UDSS_LLR_0051`` makes the sending entity's. It is not derived from the request,
    ///   because a functionally addressed request's `S_TA` is the functional group
    ///   address.
    /// * `params` - the session layer's [`ServerParams`]: `tS3_Server` and the
    ///   response-pending lead, which the server enforces, and the `P2Server_max` and
    ///   `P2*Server_max` it enforces only where `services` has no
    ///   `DiagnosticSessionControl`; otherwise each session's
    ///   [`DiagnosticSessionControl::timing`](crate::DiagnosticSessionControl::timing)
    ///   replaces them
    pub const fn new(
        services: A::App,
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
            pending: Unconfirmed::NONE,
            unmade: None,
            idle: true,
            params,
            started: false,
        }
    }

    /// The application's services, for whatever it needs them for between events.
    pub fn services(&mut self) -> &mut A::App {
        &mut self.services
    }

    /// The transport, for inspection between events.
    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The transport, to configure it between events: before the first step, say, once
    /// the target knows what it could not know when it built the server. `None` while a
    /// session change or a reset awaits its response's confirmation, on which it takes
    /// effect.
    ///
    /// Do not take the transport's events through it: an event taken there, a
    /// confirmation or a request, never reaches the server. Nor replace the transport or
    /// close its connections while any response awaits its confirmation, or the
    /// server's record of what it awaits no longer matches the transport's.
    pub fn transport_mut(&mut self) -> Option<&mut T> {
        self.pending
            .awaited
            .is_none()
            .then_some(&mut self.transport)
    }

    /// Run [`ServiceSet::start_up`] on the first step, and again on the first step after a
    /// [`Stopped`].
    fn start_up(&mut self) {
        if !core::mem::replace(&mut self.started, true) {
            A::start_up(&mut self.services, &mut self.state);
        }
    }

    /// Handle one transport event. Returns [`Stepped::Stopped`] where a restarting reset
    /// was confirmed during this step, which the caller acts on before stepping again.
    ///
    /// An `Err` says whether this instance may be stepped again (``UDSSVC_ARCH_0040``):
    ///
    /// - [`StepError::Recoverable`]: the transport failed while the server awaited its
    ///   next event between requests, where nothing is half done. A response awaiting
    ///   its confirmation keeps its change, which that confirmation makes, and `step` may
    ///   be called again.
    /// - [`StepError::Terminal`]: the transport failed while an event or a request was
    ///   handled. The error unwinds through the drains, so a session-hook decision a
    ///   drain recorded (a `tS3_Server` expiry, a confirmed session) may not have been
    ///   applied, and the application's state and the session layer may disagree
    ///   afterwards. **Do not call `step` on this instance again**; [`Self::into_parts`]
    ///   takes the services and the transport back out.
    ///
    /// # Cancel safety
    ///
    /// The returned future is **not** cancel-safe: drop it only between `step` calls.
    /// Dropped at any await other than the transport's `next_event` between requests, it
    /// leaves the instance in the state a [`StepError::Terminal`] does.
    ///
    /// # Errors
    ///
    /// [`StepError`], carrying the [`UdsTransport::Error`], where the transport failed. A
    /// negative response is not an error: it is a response, written into the sink.
    ///
    /// # Panics
    ///
    /// Where [`DiagnosticSessionControl::timing`](crate::DiagnosticSessionControl::timing)
    /// returns a pair the [`ServerParams`] response-pending lead does not fit
    /// ([`ServerParams::lead_fits`]), on the first request that pair would time.
    pub async fn step(&mut self) -> Result<Stepped, StepError<T::Error>> {
        let step = Stepping(self);
        let handled = match step.0.handle().await {
            Ok(handled) => handled,
            Err(error) if step.0.idle => return Err(StepError::Recoverable(error)),
            Err(error) => return Err(StepError::Terminal(error)),
        };
        step.0.idle = true;
        let Some(stopped) = handled else {
            return Ok(Stepped::Continue);
        };
        // Annex I transition 1 owes start-up again after a restart.
        step.0.started = false;
        step.0.transport.restarting().await;
        Ok(Stepped::Stopped(stopped))
    }

    /// The body of [`Self::step`].
    async fn handle(&mut self) -> Result<Option<Stopped>, T::Error> {
        self.start_up();
        let Buffers {
            in_flight,
            concurrent,
            response,
        } = self.store.split();
        let deadline = self.session.next_deadline();
        let ev = between_requests(&mut self.transport, &mut self.idle, in_flight, deadline);
        let ev = ev.await?;
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
                return Ok(apply::<A>(&mut self.services, &mut self.state, d.deferred));
            }
            TransportEvent::Deadline => {
                let reaction = self.session.tick(now);
                let d =
                    drain(reaction, &mut self.transport, &mut self.pending, None).await?;
                return Ok(apply::<A>(&mut self.services, &mut self.state, d.deferred));
            }
            // Periodic has no consumer; a Closed outside a handler ends nothing here.
            TransportEvent::Periodic { .. } | TransportEvent::Closed { .. } => {
                return Ok(None);
            }
        };

        let outbound_max = self.transport.outbound_max();
        let mut stopped = retime::<A, _, PEERS>(
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
        stopped = stopped.or(apply::<A>(&mut self.services, &mut self.state, d.deferred));
        let Some((ai, _)) = d.indication else {
            return Ok(stopped);
        };
        // 10.3.1: no request is served between a restart's response and its confirmation,
        // nor after the restart within this step. `stopped` is `None` here for as long as
        // uds_session yields no `Confirm` from a tick, a parameter change or an
        // indication; the check keeps 10.3.1 if one ever confirms a restart.
        if stopped.is_some() || restarting(self.pending.awaited) {
            return Ok(stopped.or(self.unanswered(ai).await?));
        }
        // The byte is kept as received because a negative response echoes it
        // (ISO 14229-1:2020 Table 21, SIDRQ). An empty request is the pipeline's to
        // settle, and settles without pending, so the 0 is never transmitted.
        let sid = received.bytes().first().copied().unwrap_or(0);
        let serving = Serving::new::<A>(self.own, ai, sid);

        let mut sink = ResponseSink::new(response, outbound_max);
        // Scoped, because `pin!` binds to the enclosing block: `handler` drops before
        // `sink` is settled and read.
        let served = {
            let handler = core::pin::pin!(A::dispatch(
                &mut self.services,
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
        self.unmade = Some(served.effect()).filter(|e| *e != Effect::None);
        // Recorded mid-handler, applied now that `&mut services`/`&mut state` are free.
        stopped = stopped.or(apply::<A>(
            &mut self.services,
            &mut self.state,
            served.deferred,
        ));
        // `stopped` is `None` here for as long as `pending` is no restart while a handler
        // runs, which the check before dispatch ensures and only `answer` changes. A
        // restart confirmed under the handler would end its request with no response.
        if stopped.is_some() {
            return Ok(stopped.or(self.unanswered(serving.ai).await?));
        }
        let serving = serving.leaving_if::<A>(&self.services, &served);
        let concluded = conclude::<A, _, PEERS>(
            &mut self.session,
            &mut self.transport,
            &mut self.pending,
            &mut concurrent[..],
            served,
            serving,
            &mut sink,
        )
        .await?;
        let deferred = self.hand_over(concluded);
        Ok(stopped.or(apply::<A>(&mut self.services, &mut self.state, deferred)))
    }

    /// Stop tracking [`Self::unmade`] where `concluded` handed it over, from which on it
    /// is confirmed or abandoned; one lost is left for [`Stepping`] to abandon. Returns
    /// what the conclusion recorded.
    fn hand_over(&mut self, concluded: Concluded) -> Deferred {
        if concluded.handed_over {
            self.unmade = None;
        }
        concluded.deferred
    }

    /// Abandon what [`Stepping`] finds unmade: [`Self::unmade`], the change whose
    /// confirmation failed and, where the step ended anywhere but between requests, the
    /// change awaiting its confirmation.
    #[cold]
    #[inline(never)]
    fn abandon_unmade(&mut self) {
        let unconfirmed = if self.idle {
            None
        } else {
            self.pending.awaited.take().map(|p| p.effect)
        };
        let failed = self.pending.failed.take();
        for effect in [self.unmade.take(), failed, unconfirmed]
            .into_iter()
            .flatten()
        {
            A::abandoned(&mut self.services, &self.state, effect);
        }
    }

    /// ``UDSS_LLR_0074`` — end the request `ai` with no response, and apply what its
    /// completion report's drain recorded.
    async fn unanswered(&mut self, ai: Ai) -> Result<Option<Stopped>, T::Error> {
        let d = complete(
            &mut self.session,
            &mut self.transport,
            &mut self.pending,
            ai,
            Effect::None,
        )
        .await?;
        Ok(apply::<A>(&mut self.services, &mut self.state, d))
    }

    /// Run until a restarting reset is confirmed, or the transport fails.
    ///
    /// The `Err` this returns is [`Self::step`]'s. After a [`StepError::Recoverable`],
    /// `run` or `step` may be called on this instance again. After a
    /// [`StepError::Terminal`], **do not call `run` or `step` on this instance again**;
    /// [`Self::into_parts`] takes the services and the transport back out.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe: dropping this future drops a `step` future, and not necessarily
    /// at the transport's `next_event`. To stop between events, drive `step` in your own
    /// loop.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returned.
    ///
    /// # Panics
    ///
    /// Where [`Self::step`] does.
    pub async fn run(&mut self) -> Result<Stopped, StepError<T::Error>> {
        loop {
            if let Stepped::Stopped(stopped) = self.step().await? {
                return Ok(stopped);
            }
        }
    }

    /// The services and the transport, for a caller that rebuilds the server after
    /// [`Stopped`] (to bind again with a new identity, say) without losing the services'
    /// state. The caller holds the `own` address and [`ServerParams`] it built this with.
    ///
    /// A server in a `ConstStaticCell`/`StaticCell` cannot be moved out of; it is stepped
    /// again instead. After a [`StepError::Terminal`], this is the only supported use.
    #[must_use]
    pub fn into_parts(self) -> (A::App, T) {
        (self.services, self.transport)
    }
}

/// [`Server::step`]'s drop guard (``UDSSVC_ARCH_0046``): abandons what the step leaves
/// unmade, whether it returns, fails or is dropped. That is the change a handler accepted
/// whose response was neither submitted nor completed, the change whose confirmation a
/// drain reported failed and, where the step ends anywhere but between requests, the
/// change whose response awaits its confirmation, which an ended instance never takes.
struct Stepping<'s, A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    &'s mut Server<A, T, PEERS>,
);

impl<A: ServiceSet, T: UdsTransport, const PEERS: usize> Drop
    for Stepping<'_, A, T, PEERS>
{
    fn drop(&mut self) {
        let pending = self.0.pending;
        if self.0.unmade.is_some()
            || pending.failed.is_some()
            || (!self.0.idle && pending.awaited.is_some())
        {
            self.0.abandon_unmade();
        }
    }
}

/// What one [`Server::step`] ended with.
#[must_use = "a `Stepped::Stopped` asks the caller to act on a restart before stepping again"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stepped {
    /// The event was handled, and the server serves on.
    Continue,
    /// The server handed control back; see [`Stopped`].
    Stopped(Stopped),
}

/// Why [`Server::step`] or [`Server::run`] handed control back (``UDSSVC_ARCH_0045``).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// A reset whose [`EcuReset::reset`](crate::EcuReset::reset) said
    /// [`AfterReset::Restart`] was confirmed: the protocol state is the power-up one, and
    /// what follows (a reboot delay, binding again, a new identity) is the caller's. The
    /// server may be stepped again, which runs start-up again, or taken apart with
    /// [`Server::into_parts`].
    Reset(ResetType),
}

/// The transport's next event, awaited with `idle` set, and left set where it fails: the
/// one await [`Server::step`] is dropped at, or fails at, with the server serving on.
async fn between_requests<'b, T: UdsTransport>(
    transport: &mut T,
    idle: &mut bool,
    buffer: &'b mut [u8],
    deadline: Option<Timestamp>,
) -> Result<TransportEvent<'b>, T::Error> {
    *idle = true;
    let event = transport.next_event(buffer, deadline).await?;
    *idle = false;
    Ok(event)
}

/// Why [`Server::step`] or [`Server::run`] failed: the transport's error, and whether the
/// server may be stepped again (``UDSSVC_ARCH_0040``).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError<E> {
    /// The transport failed while the server awaited its next event between requests.
    /// Nothing was half done there: a response awaiting its confirmation keeps its
    /// change, and the server may be stepped again.
    Recoverable(E),
    /// The transport failed while an event or a request was handled. Terminal for the
    /// instance; see [`Server::step`].
    Terminal(E),
}

impl<E> StepError<E> {
    /// Whether the server may be stepped again.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Recoverable(_))
    }

    /// The transport's error.
    #[must_use]
    pub fn into_inner(self) -> E {
        match self {
            Self::Recoverable(error) | Self::Terminal(error) => error,
        }
    }
}

impl<E: core::fmt::Debug> core::fmt::Display for StepError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Recoverable(error) => {
                write!(f, "the transport failed between requests: {error:?}")
            }
            Self::Terminal(error) => {
                write!(f, "the transport failed mid-request: {error:?}")
            }
        }
    }
}

impl<E: core::fmt::Debug> core::error::Error for StepError<E> {}

/// Load the `P2` pair of the session in force into the session layer, so the request about
/// to be indicated at `now` opens the window its session advertised. The session layer is
/// ticked at `now` first, so a `tS3_Server` running out at that instant returns to the
/// default session before its pair is read; ``UDSS_LLR_0076`` leaves any window already
/// open on the value it was loaded with.
///
/// # Panics
///
/// Where the pair leaves `params`' response-pending lead ill formed: the enhanced overrun
/// would then come due before ``UDSS_LLR_0119`` admits its 0x78, so the server could not
/// keep the window it advertised.
async fn retime<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    services: &mut A::App,
    state: &mut A::State,
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    params: ServerParams,
    now: Timestamp,
) -> Result<Option<Stopped>, T::Error> {
    let tick = drain(session.tick(now), transport, pending, None).await?;
    let mut stopped = apply::<A>(services, state, tick.deferred);
    let Some(timing) = A::session_timing(services, state) else {
        return Ok(stopped);
    };
    assert!(
        params.lead_fits(timing.p2_server_max(), timing.p2_star_server_max()),
        "DiagnosticSessionControl::timing returned a pair the response-pending lead does \
         not fit (ServerParams::lead_fits)",
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
        stopped = stopped.or(apply::<A>(services, state, d.deferred));
    }
    Ok(stopped)
}

/// The response in flight that carries an effect, such as a `DiagnosticSessionControl`
/// response's session: the effect takes place on the `Confirm` that reports it sent
/// (``UDSS_LLR_0085``). One slot, which is why [`Server::new`] accepts `PEERS == 1` only:
/// a server answers one request at a time (``UDSS_LLR_0108``), but a response awaiting
/// its confirmation is not the request in progress, and a second tester's selecting
/// response could overwrite this slot before the first was confirmed.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// The response's addressing, exactly as submitted: `t_data_conf` matches by it.
    ai: Ai,
    /// What the response does once confirmed.
    effect: Effect,
}

/// What waits on a confirmation (``UDSS_LLR_0085``): the response in flight, and the
/// change of one whose confirmation failed, which [`Stepping`] abandons as the step ends,
/// whether it returns, fails or is dropped.
#[derive(Debug, Clone, Copy)]
struct Unconfirmed {
    /// The response awaiting its confirmation, if any.
    awaited: Option<Pending>,
    /// The change of the response whose confirmation failed, until it is abandoned.
    failed: Option<Effect>,
}

impl Unconfirmed {
    const NONE: Self = Self {
        awaited: None,
        failed: None,
    };
}

/// Where a response to `request` goes: the `Ai` `s_data_req` registers, and so the one
/// the response's `DataConf` carries to match it. Sent from `own`, never from the
/// request's `S_TA`, which for a functional request is the group address
/// (``UDSS_LLR_0051``), and physically addressed whatever the request was, since a server
/// answers the one client that asked (``UDSS_LLR_0026``).
const fn reply_address(own: Address, request: Ai) -> Ai {
    Ai {
        sa: own,
        ta: request.sa,
        ta_type: TaType::Physical,
        ..request
    }
}

/// What a drain found that the application must hear about, applied by [`apply`] where
/// `&mut services`/`&mut state` are free.
#[derive(Debug, Clone, Copy)]
struct Deferred {
    /// `tS3_Server` expired (``UDSS_LLR_0100``).
    timed_out: bool,
    /// A response with an effect was confirmed sent, or its suppression completed.
    confirmed: Option<Effect>,
    /// A confirmed session a later timeout overtook.
    abandoned: Option<Effect>,
}

impl Deferred {
    const NONE: Self = Self {
        timed_out: false,
        confirmed: None,
        abandoned: None,
    };

    /// Both records, in arrival order: `self` was recorded first.
    ///
    /// A later timeout abandons an earlier session confirmation, since [`apply`] runs the
    /// timeout first and the session layer has expired the session. A confirmed reset is
    /// kept: its response was sent, and both end in the default session. A later
    /// confirmation after an earlier timeout keeps both; of two confirmations the later
    /// wins.
    // Out of line: inlined at each of its many call sites, it adds over a kilobyte of
    // text to the embedded probe.
    #[inline(never)]
    fn merge(self, later: Self) -> Self {
        let overtaken = self
            .confirmed
            .filter(|e| later.timed_out && matches!(e, Effect::Session(_)));
        Self {
            abandoned: later.abandoned.or(self.abandoned).or(overtaken),
            timed_out: self.timed_out || later.timed_out,
            confirmed: if later.timed_out {
                // A confirmed reset happened whatever expired after it: its response was
                // sent, and both end in the default session.
                later
                    .confirmed
                    .or(self.confirmed.filter(|e| matches!(e, Effect::Reset { .. })))
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
/// everything else, a 0x78 to the same addressing and a negative response included, as
/// neither starts with the positive response identifier.
fn after_send(leaving: Option<Ai>, ai: Ai, data: &[u8]) -> AfterSend {
    let positive = UdsServiceType::DiagnosticSessionControl.to_response_sid();
    if leaving == Some(ai) && data.first() == Some(&positive) {
        AfterSend::ServerLeaves
    } else {
        AfterSend::Continue
    }
}

/// The one place every session output is handled (``UDSSVC_ARCH_0040``): every reaction,
/// from every input, passes through here, because an expiry can surface from any of
/// them. Acts on what needs only the transport and the pending record, every `Transmit`
/// included, and records the rest. The reaction borrows the session and `t_data_req`
/// borrows the transport, so they arrive as separate arguments.
async fn drain<'d, T: UdsTransport, const PEERS: usize>(
    mut reaction: uds_session::ServerReaction<'_, 'd, PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
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
                if let Some(p) = pending.awaited
                    && p.ai == ai
                {
                    pending.awaited = None;
                    if result == SResult::Ok {
                        found.deferred.confirmed = Some(p.effect);
                    } else {
                        pending.failed = Some(p.effect);
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

/// Act on what a drain recorded: the timeout, then the abandoned change, then the
/// confirmation. The only caller of the session hooks, so the protocol state's session is
/// written only here: when `tS3_Server` times out, and when the response that selected a
/// session or accepted a restarting reset is confirmed or its completion is reported
/// (``UDSSVC_ARCH_0002``, ``UDSSVC_ARCH_0038``, ``UDSSVC_ARCH_0045``). Returns `Some`
/// where that reset restarts.
///
/// That fixed order is the arrival order: within one drain ``UDSS_LLR_0081`` puts the
/// expiry snapshots ahead of the input's own outputs, across drains [`Deferred::merge`]
/// abandons a session confirmation that a later timeout overtook, and a change abandoned
/// writes no session.
fn apply<A: ServiceSet>(
    services: &mut A::App,
    state: &mut A::State,
    d: Deferred,
) -> Option<Stopped> {
    if d.timed_out {
        A::session_timed_out(services, state);
    }
    if let Some(effect) = d.abandoned {
        A::abandoned(services, state, effect);
    }
    match d.confirmed {
        Some(Effect::Session(selected)) => {
            A::session_confirmed(services, state, selected);
            None
        }
        Some(Effect::Reset { kind, after }) => {
            A::reset_confirmed(services, state, kind, after);
            matches!(after, AfterReset::Restart).then_some(Stopped::Reset(kind))
        }
        Some(Effect::None) | None => None,
    }
}

/// Whether a `Restart` response is awaiting its confirmation, during which 10.3.1
/// recommends the server accept no request.
const fn restarting(pending: Option<Pending>) -> bool {
    matches!(
        pending,
        Some(Pending {
            effect: Effect::Reset {
                after: AfterReset::Restart,
                ..
            },
            ..
        })
    )
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
    /// ``UDSSVC_ARCH_0032``: whether the service admits a 0x78 at all, as
    /// `ServiceSet::may_respond_pending` reported it.
    may_pend: bool,
    /// `reply_to`, where the final positive response selects a session that
    /// `ServiceSet::leaves_running_software`: the one message sent
    /// [`AfterSend::ServerLeaves`]. Known once the handler has finished.
    leaving: Option<Ai>,
}

impl Serving {
    /// The request `ai` sent, its service identifier `sid`, before it is dispatched.
    /// ``UDSSVC_ARCH_0032``: whether 0x78 is admissible is the service's to say.
    fn new<A: ServiceSet>(own: Address, ai: Ai, sid: u8) -> Self {
        Self {
            ai,
            own,
            reply_to: reply_address(own, ai),
            sid,
            may_pend: A::may_respond_pending(UdsServiceType::from_request_sid(sid)),
            leaving: None,
        }
    }

    /// The same, with [`Self::leaving`] set where `served` selects a session that
    /// `services` leave the running software to enter.
    fn leaving_if<A: ServiceSet>(self, services: &A::App, served: &Served) -> Self {
        Self {
            leaving: match served.effect() {
                Effect::Session(selected)
                    if A::leaves_running_software(services, selected) =>
                {
                    Some(self.reply_to)
                }
                _ => None,
            },
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
    /// The handler's effect, where it finished positively.
    fn effect(&self) -> Effect {
        match self.ended {
            Ended::Finished(unsettled) => unsettled
                .parts()
                .and_then(|(_, outcome)| outcome.ok())
                .unwrap_or(Effect::None),
            Ended::Closed => Effect::None,
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
/// `&mut services`/`&mut state`. The caller settles, once the handler has released the
/// sink. A message arriving meanwhile finds the protocol instance occupied (ISO
/// 14229-1:2020 8.7.6): the keep-alive `TesterPresent` bypasses it ([`keep_alive`]) and
/// anything else is refused ([`refuse_busy`]). The requesting client's close ends the
/// handling; another client's ends nothing. Every other event first answers an overrun due
/// by its own timestamp ([`overrun_at`]), so none is lost to the event's own input.
async fn serve<
    A: ServiceSet,
    H: Future<Output = Unsettled>,
    T: UdsTransport,
    const PEERS: usize,
>(
    mut handler: Pin<&mut H>,
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    concurrent: &mut [u8],
    serving: Serving,
) -> Result<Served, T::Error> {
    let mut deferred = Deferred::NONE;
    let mut pending_sent = false;
    let mut owed = false;
    // A loop over `handler.as_mut()`: the handler survives each deadline it loses.
    loop {
        let deadline = session.next_deadline();
        let event = {
            let waiting = core::pin::pin!(transport.next_event(concurrent, deadline));
            select2(handler.as_mut(), waiting).await
        };
        // An `Err` at any `?` below drops what was recorded; see `Server::step`'s doc.
        match event {
            Either::Left(unsettled) => {
                return Ok(Served {
                    ended: Ended::Finished(unsettled),
                    pending_sent,
                    deferred,
                });
            }
            Either::Right(Err(e)) => return Err(e),
            Either::Right(Ok(TransportEvent::Closed { peer, .. }))
                if peer == serving.ai.sa =>
            {
                // The exchange is over: a server does not reconnect (ISO 14229-5:2022
                // REQ 7.8 and REQ 7.10 have the client reconnect). Nothing positive has
                // been sent yet, so this close is never REQ 7.9's or 7.11's, and whether
                // it was expected changes nothing.
                return Ok(Served {
                    ended: Ended::Closed,
                    pending_sent,
                    deferred,
                });
            }
            Either::Right(Ok(event)) => {
                let now = transport.now();
                let o = overrun_at(session, transport, pending, now, serving).await?;
                // UDSSVC_ARCH_0009 rule 3, kept for `settle`: one accepted 0x78 is enough.
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
                    // consumer; another peer's close ends nothing here.
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
    pending: &mut Unconfirmed,
    now: Timestamp,
    ai: Ai,
    data: &[u8],
) -> Result<Deferred, T::Error> {
    let reaction = session.t_data_ind(now, ai, data, SResult::Ok, ServerRx::KeepAlive);
    Ok(drain(reaction, transport, pending, None).await?.deferred)
}

/// ISO 14229-1:2020 8.7.6: any other message arriving while a service is in progress
/// finds the protocol instance occupied and is refused `busyRepeatRequest` (Annex A) from
/// its service identifier alone; the service in progress continues. Never indicated as a
/// request (``UDSS_LLR_0108``), and submitted as a busy refusal, which answers no service
/// (``UDSS_LLR_0187``). A refusal the session layer refuses in turn is dropped, which is
/// Annex J Figure J.2's other branch; so is a message with no service identifier
/// (``UDSSVC_ARCH_0005``).
async fn refuse_busy<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
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

/// What [`conclude`] recorded, and whether it handed the request's own change over: its
/// response submitted into `pending`, or its completion reported with it selected.
/// Matched by this request alone, so an earlier identical change in `pending` or
/// confirmed meanwhile never stands in for one whose response was lost.
#[derive(Debug, Clone, Copy)]
struct Concluded {
    deferred: Deferred,
    handed_over: bool,
}

/// End the request once its handling has: settle and answer it, or, where the link closed
/// first, report it over with nothing selected (``UDSS_LLR_0074``), no response being
/// sendable and none having been decided against.
async fn conclude<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    concurrent: &mut [u8],
    served: Served,
    serving: Serving,
    sink: &mut ResponseSink<'_>,
) -> Result<Concluded, T::Error> {
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
        Ended::Closed => Ok(Concluded {
            deferred: complete(session, transport, pending, serving.ai, Effect::None)
                .await?,
            handed_over: false,
        }),
    }
}

/// Submit the handler's outcome: a final response, or the completion report a
/// suppressed request owes. Returns what the application must hear about, for the caller
/// to [`apply`], and whether the outcome's change was handed over.
///
/// A final response refused under ``UDSS_LLR_0061`` (the 0x78's `DataConf` not yet
/// drained) or ``UDSS_LLR_0062`` (every association awaiting a confirmation) is waited
/// out and resubmitted once. A refusal for any other cause loses the response: the tester
/// has had at most a 0x78, and the service stays in progress until its timer runs out.
async fn answer<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    concurrent: &mut [u8],
    outcome: Responded,
    serving: Serving,
    response: &[u8],
) -> Result<Concluded, T::Error> {
    let effect = outcome.effect();
    if let Responded::Suppressed { .. } = outcome {
        return Ok(Concluded {
            deferred: complete(session, transport, pending, serving.ai, effect).await?,
            handed_over: effect != Effect::None,
        });
    }
    let now = transport.now();
    let reaction =
        final_response(session, now, serving.reply_to, response, selection(effect));
    // An `Err` at any `?` below drops what was recorded; see `Server::step`'s doc.
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
            let closed =
                complete(session, transport, pending, serving.ai, Effect::None).await?;
            return Ok(Concluded {
                deferred: deferred.merge(closed),
                handed_over: false,
            });
        };
        deferred = deferred.merge(waited);
        let now = transport.now();
        let reaction =
            final_response(session, now, serving.reply_to, response, selection(effect));
        d = drain(reaction, transport, pending, serving.leaving).await?;
        deferred = deferred.merge(d.deferred);
    }
    // Recorded only once the submission is accepted, so a refused response never leaves
    // a pending selection behind, and never disturbs one already in flight. `reply_to`
    // is the association `t_data_conf` will match. UDSSVC_ARCH_0040: a final response
    // refused for any other cause, or refused again after waiting one out, is lost. See
    // `answer`'s doc.
    let handed_over = d.rejected.is_none() && effect != Effect::None;
    if handed_over {
        pending.awaited = Some(Pending {
            ai: serving.reply_to,
            effect,
        });
    }
    Ok(Concluded {
        deferred,
        handed_over,
    })
}

/// The final response's submission, built in one place because [`answer`] may make it
/// twice.
fn final_response<'s, 'd, const PEERS: usize>(
    session: &'s mut SessionServer<PEERS>,
    now: Timestamp,
    reply_to: Ai,
    response: &'d [u8],
    selection: Option<uds_session::SessionSelection>,
) -> uds_session::ServerReaction<'s, 'd, PEERS> {
    session.s_data_req(
        now,
        reply_to,
        response,
        ServerTx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: selection,
        },
    )
}

/// The session the session layer is told a response or completion report selects
/// (``UDSS_LLR_0085``/``0086``).
fn selection(effect: Effect) -> Option<uds_session::SessionSelection> {
    match effect {
        Effect::Session(session) => Some(selection_of(session)),
        Effect::Reset {
            after: AfterReset::Restart,
            ..
        } => Some(uds_session::SessionSelection::Default),
        Effect::Reset { .. } | Effect::None => None,
    }
}

/// ``UDSS_LLR_0074`` — report a request complete with no response, and with it the
/// selection it made.
///
/// ``UDSS_LLR_0086`` / ``0098`` — the completion report is the moment, so the selection
/// follows whatever the report's own drain recorded.
async fn complete<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    ai: Ai,
    effect: Effect,
) -> Result<Deferred, T::Error> {
    let now = transport.now();
    let class = ServerRx::Request {
        session: selection(effect),
    };
    let reaction = session.completion_report(now, ai, class);
    let d = drain(reaction, transport, pending, None).await?;
    Ok(d.deferred.merge(Deferred {
        timed_out: false,
        confirmed: (effect != Effect::None).then_some(effect),
        abandoned: None,
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
/// ``UDSS_LLR_0061`` or ``UDSS_LLR_0062`` refused can be resubmitted. Each event takes
/// the path it takes mid-handler: an overrun due by its timestamp is answered first
/// ([`overrun_at`]), a `DataConf` reaches `t_data_conf`, and a message arriving is
/// bypassed or refused as in [`serve`]. A 0x78 refused here is not owed: the final
/// response supersedes it.
///
/// Returns what the drains recorded once the awaited `DataConf` has been drained and
/// accepted, or `None` where the requesting client's link closed first. The wait is
/// unbounded (``UDSSVC_ARCH_0040``): it ends on the awaited `DataConf`, that client's
/// `Closed`, or a transport error, which [`UdsTransport`]'s obligations make certain.
async fn await_confirmation<A: ServiceSet, T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
    concurrent: &mut [u8],
    serving: Serving,
    awaited: Awaited,
) -> Result<Option<Deferred>, T::Error> {
    let mut deferred = Deferred::NONE;
    loop {
        let deadline = session.next_deadline();
        let event = transport.next_event(concurrent, deadline).await?;
        if let TransportEvent::Closed { peer, .. } = event
            && peer == serving.ai.sa
        {
            return Ok(None);
        }
        // The handler's outcome is already settled, so whether a 0x78 goes out does
        // not reach `settle`; only what the drains recorded is kept.
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
    pending: &mut Unconfirmed,
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
/// ``UDSSVC_ARCH_0031``/``0032``: the deadline passing is not itself the overrun; the
/// session layer is ticked and ``UDSS_LLR_0117``'s `ResponseOverrun` is the output that
/// says a 0x78 is owed. It is an ordinary transmission, and whether it was *accepted* is
/// the verdict ``UDSSVC_ARCH_0009`` rule 3 reads.
async fn answer_overrun<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
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
/// client's addressing awaits a confirmation (a busy refusal's, or a final response's
/// to another request) is reported `owed`, for the caller to resubmit once a
/// confirmation has freed it.
async fn submit_pending<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Unconfirmed,
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
    use crate::services::Effect;
    use uds_protocol::DiagnosticSessionType as S;
    use uds_session::{Address, Ai, Mtype, TaType};

    const TO_TESTER: Ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0E00),
        ta: Address(0x0E80),
        ta_type: TaType::Physical,
    };

    /// ISO 14229-5:2022 REQ 7.9 closes the connection after the positive
    /// `DiagnosticSessionControl` response of a session change that disconnects it, so
    /// only that response to the leaving request's addressing is `ServerLeaves`: never the
    /// 0x78 sharing that addressing, a negative response, another tester's response, or
    /// anything sent while no session change leaves.
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
        ..Deferred::NONE
    };

    const fn confirm(session: S) -> Deferred {
        Deferred {
            confirmed: Some(Effect::Session(session)),
            ..Deferred::NONE
        }
    }

    /// A confirmation recorded mid-handler and then overtaken by a `tS3_Server` expiry
    /// is abandoned.
    #[test]
    fn a_later_timeout_abandons_an_earlier_confirmation() {
        let merged = confirm(S::ExtendedDiagnosticSession).merge(TIMEOUT);
        assert!(merged.timed_out);
        assert_eq!(merged.confirmed, None);
        assert_eq!(
            merged.abandoned,
            Some(Effect::Session(S::ExtendedDiagnosticSession))
        );
    }

    /// A confirmation after a timeout keeps both, and `apply`'s fixed order is then
    /// their arrival order.
    #[test]
    fn a_later_confirmation_after_a_timeout_keeps_both() {
        let merged = TIMEOUT.merge(confirm(S::ExtendedDiagnosticSession));
        assert!(merged.timed_out);
        assert_eq!(
            merged.confirmed,
            Some(Effect::Session(S::ExtendedDiagnosticSession))
        );
    }

    /// Of two confirmations, the later is the session in force.
    #[test]
    fn of_two_confirmations_the_later_wins() {
        let merged =
            confirm(S::ExtendedDiagnosticSession).merge(confirm(S::DefaultSession));
        assert!(!merged.timed_out);
        assert_eq!(merged.confirmed, Some(Effect::Session(S::DefaultSession)));
    }
}
