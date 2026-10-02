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

use crate::ResponseSink;
use crate::select::{Either, select2};
use crate::services::{Responded, ServiceSet};
use crate::state::ProtocolState;
use crate::storage::{Buffers, Storage};
use crate::transport::{TransportEvent, UdsTransport};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use uds_protocol::{DiagnosticSessionType, UdsServiceType};
pub use uds_session::ServerParams;
use uds_session::{
    Address, Ai, Association, Cause, Rejection, SResult, Server as SessionServer,
    ServerOutput, ServerRx, ServerTx, SessionSelection, Solicitation, TaType, Timestamp,
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
}

impl<A: ServiceSet, T: UdsTransport, const PEERS: usize> Server<A, T, PEERS> {
    /// Assemble a server.
    ///
    /// A `const fn`, which is load-bearing: the storage is inline and can be several
    /// kilobytes, so a runtime constructor would build a stack temporary before the move
    /// and a small-stack target could not hold it. `static SERVER: EcuServer =
    /// EcuServer::new(..)` constructs in place, which is what
    /// [`Storage::EMPTY`] being an associated const buys.
    ///
    /// The driver builds its own session, so an application never names `uds_session`.
    ///
    /// `PEERS` must be 1, checked at compile time: milestone 1 keeps one slot for a
    /// selecting response awaiting its confirmation (see [`crate::uds_server`]).
    ///
    /// `own` is the address every response is sent from: its `S_SA`, which
    /// ``UDSS_LLR_0051`` makes the sending entity's. It is not derived from the request,
    /// because a functionally addressed request's `S_TA` is the functional group address,
    /// and a response sent from that address would not say which server answered.
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

    /// Handle one transport event.
    ///
    /// **An `Err` is terminal for this server instance** (``UDSSVC_ARCH_0040``,
    /// milestone-1 limit). A transport error unwinds through the drains with `?`, so a
    /// session-hook decision a drain already recorded — a `tS3_Server` expiry, a
    /// confirmed session — may not have been applied, and the application's state and
    /// the session layer may disagree afterwards. The caller recreates the server rather
    /// than calling `step` again.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed. A negative response is not an
    /// error: it is a response, written into the sink.
    pub async fn step(&mut self) -> Result<(), T::Error> {
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
        let (ai, request_bytes) = match ev {
            TransportEvent::DataInd { ai, data } => (ai, data),
            TransportEvent::DataConf { ai, result } => {
                // UDSS_LLR_0063 — one matching no association is rejected by the session
                // layer; the drain reads the verdict and nothing changes.
                let reaction = self.session.t_data_conf(now, ai, result);
                let d = drain(reaction, &mut self.transport, &mut self.pending).await?;
                apply(&mut self.services, &mut self.state, d.deferred);
                return Ok(());
            }
            TransportEvent::Deadline => {
                let reaction = self.session.tick(now);
                let d = drain(reaction, &mut self.transport, &mut self.pending).await?;
                apply(&mut self.services, &mut self.state, d.deferred);
                return Ok(());
            }
            // Elided: a DataTooLong here exceeds this entity's own MDS and owes 0x21 or
            // 0x13 (architecture open question 1); Periodic has no consumer; a Closed
            // outside a handler ends nothing here.
            TransportEvent::DataTooLong { .. }
            | TransportEvent::Periodic { .. }
            | TransportEvent::Closed { .. } => return Ok(()),
        };

        let outbound_max = self.transport.outbound_max();
        let reaction = self.session.t_data_ind(
            now,
            ai,
            request_bytes,
            SResult::Ok,
            ServerRx::Request { session: None },
        );
        // The drain hands the `Indicate` back: a `ServerOutput<'d>` outlives the reaction
        // that yielded it, which is what lets dispatch run with `&mut session` free.
        // Required, or the 0x78 window does not exist.
        let d = drain(reaction, &mut self.transport, &mut self.pending).await?;
        apply(&mut self.services, &mut self.state, d.deferred);
        let Some((ai, request)) = d.indication else {
            return Ok(());
        };
        // The byte is kept as received because a negative response echoes it
        // (ISO 14229-1:2020 Table 21, SIDRQ). An empty request is the pipeline's to
        // settle, and settles without pending, so the 0 is never transmitted.
        let sid = request.first().copied().unwrap_or(0);
        // UDSSVC_ARCH_0032 — whether 0x78 is admissible is the service's to say, never the
        // driver's. Resolved before dispatch, which holds `&mut services` until it settles.
        let may_pend = self
            .services
            .may_respond_pending(UdsServiceType::from_request_sid(sid));
        let serving = Serving {
            ai,
            reply_to: reply_address(self.own, ai),
            sid,
            may_pend,
        };

        let mut sink = ResponseSink::new(response, outbound_max);
        // UDSSVC_ARCH_0009 rule 3's input, fresh for this request, so a local rather
        // than a field. The driver learns the answer while the handler future is live,
        // and a shared reference is the one thing that can coexist with that future; an
        // atomic rather than a `Cell`, because that reference is held across `.await`
        // and must be `Sync` for this future to stay `Send`.
        let pending_sent = AtomicBool::new(false);
        // Scoped, because `pin!` binds to the enclosing block: `handler` drops before
        // `sink` is read.
        let (outcome, deferred) = {
            let handler = core::pin::pin!(self.services.dispatch(
                &mut self.state,
                ai,
                request,
                &mut sink,
                &pending_sent,
            ));
            serve(
                handler,
                &mut self.session,
                &mut self.transport,
                &mut self.pending,
                &mut concurrent[..],
                serving,
                &pending_sent,
            )
            .await?
        };
        // Recorded mid-handler, applied now that `&mut services`/`&mut state` are free.
        apply(&mut self.services, &mut self.state, deferred);

        let deferred = answer(
            &mut self.session,
            &mut self.transport,
            &mut self.pending,
            &mut concurrent[..],
            outcome,
            serving,
            sink.written_bytes(),
        )
        .await?;
        apply(&mut self.services, &mut self.state, deferred);
        Ok(())
    }

    /// Run until the transport fails.
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

/// The `DiagnosticSessionControl` response in flight: its session takes effect on the
/// `Confirm` that reports it sent (spec §3.2; ``UDSS_LLR_0085``). One slot: a server
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
/// ISO 14229-2:2021 9.6 (`docs/requirements/llr-service-interface.rst`, the channel
/// requirement's rationale).
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

/// The one place every session output is handled (spec §3.4). Every reaction, from every
/// input, passes through here, because an expiry can surface from any of them. Acts on
/// what needs only the transport and the pending record; records the rest.
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
            ServerOutput::Transmit { ai, data } => transport.t_data_req(ai, data).await?,
            ServerOutput::Indicate {
                ai,
                data,
                result: SResult::Ok,
            } => found.indication = Some((ai, data)),
            ServerOutput::Confirm { ai, result } => {
                // Only the selecting response's own confirmation settles the slot; a
                // failed one clears it and leaves the session where it was (spec §3.2).
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
    found.rejected = reaction.finish().err();
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
    /// The request's addressing, swapped: where its responses go.
    reply_to: Ai,
    /// The request's service identifier, which a negative response echoes.
    sid: u8,
    /// ``UDSSVC_ARCH_0032`` — whether the service admits a 0x78 at all, as
    /// `ServiceSet::may_respond_pending` reported it.
    may_pend: bool,
}

/// Poll the handler to completion, answering what the transport delivers meanwhile.
///
/// Returns the handler's outcome and what the drains it made recorded for the
/// application, which waits until the handler releases `&mut services`/`&mut state`.
/// `waiting` is scoped per iteration so it drops before the 0x78 path needs
/// `&mut transport`.
async fn serve<H: Future<Output = Responded>, T: UdsTransport, const PEERS: usize>(
    mut handler: Pin<&mut H>,
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    pending: &mut Option<Pending>,
    concurrent: &mut [u8],
    serving: Serving,
    pending_sent: &AtomicBool,
) -> Result<(Responded, Deferred), T::Error> {
    let mut deferred = Deferred::NONE;
    let mut deadline = session.next_deadline();
    // A loop over `handler.as_mut()`, not a one-shot: a plain
    // `select2(handler, waiting)` drops the handler the moment the deadline wins,
    // which is the opposite of what a response-pending is for.
    loop {
        let event = {
            let waiting = core::pin::pin!(transport.next_event(concurrent, deadline));
            select2(handler.as_mut(), waiting).await
        };
        match event {
            Either::Left(done) => return Ok((done, deferred)),
            Either::Right(Ok(TransportEvent::Deadline)) if !serving.may_pend => {
                // UDSSVC_ARCH_0032 — the service admits no 0x78, so an overrun is drained
                // as any tick is and nothing is submitted or recorded for `settle`.
                let now = transport.now();
                let reaction = session.tick(now);
                // milestone-1 limit: see `Server::step`'s doc on `Err`.
                let d = drain(reaction, transport, pending).await?;
                deferred = deferred.merge(d.deferred);
                deadline = session.next_deadline();
            }
            Either::Right(Ok(TransportEvent::Deadline)) => {
                let now = transport.now();
                // milestone-1 limit: see `Server::step`'s doc on `Err`.
                let o = answer_overrun(
                    session,
                    transport,
                    pending,
                    now,
                    serving.reply_to,
                    serving.sid,
                )
                .await?;
                if o.sent_pending {
                    // UDSSVC_ARCH_0009 rule 3 — read by `settle` when the handler
                    // settles, which may be after this instant.
                    pending_sent.store(true, Ordering::Relaxed);
                }
                deferred = deferred.merge(o.deferred);
                deadline = session.next_deadline();
            }
            Either::Right(Ok(TransportEvent::DataConf { ai, result })) => {
                // The 0x78's confirmation, or any other: it frees its association
                // and opens the enhanced window (UDSS_LLR_0116).
                let now = transport.now();
                let reaction = session.t_data_conf(now, ai, result);
                // milestone-1 limit: see `Server::step`'s doc on `Err`.
                let d = drain(reaction, transport, pending).await?;
                deferred = deferred.merge(d.deferred);
                deadline = session.next_deadline();
            }
            Either::Right(Ok(TransportEvent::Closed { expected })) => {
                // The exchange is over either way: a server does not reconnect
                // (ISO 13400-2 REQ 8.DoIP-144 puts routing activation on the
                // client). A close reaching *this* arm is never REQ 7.9's or
                // 7.11's — those follow a positive response, and nothing positive
                // has been sent yet — so `expected` needs no examination here.
                //
                // Elided: reporting it as `Suppressed` submits a completion report
                // for a request the server did not complete, where the truth is
                // that no response could be sent. Milestone 3 distinguishes them.
                let _ = expected;
                return Ok((Responded::Suppressed { session: None }, deferred));
            }
            Either::Right(Ok(_concurrent)) => {
                // Elided: a concurrent message that is not one of clause
                // 8.7.6's two exceptions (ServiceSet::is_concurrent_exception)
                // owes busyRepeatRequest (0x21), not silence — acting on the
                // classification is open question 1. A DataTooLong here is
                // occupancy of the concurrent buffer and owes 0x21 too, and
                // must never lower the advertised MDS. Periodic has no consumer.
                // This arm only re-arms the deadline.
                deadline = session.next_deadline();
            }
            Either::Right(Err(e)) => return Err(e),
        }
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
async fn answer<T: UdsTransport, const PEERS: usize>(
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
    let mut d = drain(reaction, transport, pending).await?;
    let mut deferred = d.deferred;
    let awaited = match d.rejected {
        // UDSS_LLR_0061 — only `reply_to`'s own confirmation frees its addressing.
        Some(r) if r.contains(Cause::AssociationOutstanding) => Some(Awaited::Reply),
        // UDSS_LLR_0062 — any confirmation frees an association.
        Some(r) if r.contains(Cause::NoAssociationFree) => Some(Awaited::Any),
        _ => None,
    };
    if let Some(awaited) = awaited {
        let waited =
            await_confirmation(session, transport, pending, concurrent, serving, awaited);
        let Some(waited) = waited.await? else {
            // The link closed first: reported as the mid-handler close is, `Suppressed`.
            let closed = complete(session, transport, pending, serving.ai, None).await?;
            return Ok(deferred.merge(closed));
        };
        deferred = deferred.merge(waited);
        let now = transport.now();
        let reaction = final_response(session, now, serving.reply_to, response, selected);
        d = drain(reaction, transport, pending).await?;
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
    let d = drain(reaction, transport, pending).await?;
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
/// Each event takes the path it takes mid-handler: a `DataConf` reaches `t_data_conf`,
/// a deadline reaches [`answer_overrun`] where the service admits a 0x78
/// (``UDSSVC_ARCH_0032``) and is only drained where it does not, and anything else
/// re-arms the deadline.
/// Returns what the drains recorded once the awaited `DataConf` has been drained and
/// accepted — `serving.reply_to`'s for [`Awaited::Reply`], any for [`Awaited::Any`] — or
/// `None` where the link closed first.
///
/// **Unbounded** (milestone-1 limit, ``UDSSVC_ARCH_0040``). The wait ends on the awaited
/// `DataConf`, a `Closed`, or a transport error, and on nothing else: a transport that
/// never confirms stalls the driver here. That is the assumption of use
/// [`UdsTransport`] states, a `DataConf` for every accepted `t_data_req`. Indications
/// arriving meanwhile are dropped, not answered `busyRepeatRequest` (architecture open
/// question 1).
async fn await_confirmation<T: UdsTransport, const PEERS: usize>(
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
        match transport.next_event(concurrent, deadline).await? {
            TransportEvent::DataConf { ai, result } => {
                let now = transport.now();
                let reaction = session.t_data_conf(now, ai, result);
                let d = drain(reaction, transport, pending).await?;
                deferred = deferred.merge(d.deferred);
                let matches = awaited == Awaited::Any || ai == serving.reply_to;
                if matches && d.rejected.is_none() {
                    return Ok(Some(deferred));
                }
            }
            TransportEvent::Deadline if !serving.may_pend => {
                // UDSSVC_ARCH_0032 — as in `serve`: the service admits no 0x78, so the
                // tick is drained and nothing is submitted.
                let now = transport.now();
                let reaction = session.tick(now);
                // milestone-1 limit: see `Server::step`'s doc on `Err`.
                let d = drain(reaction, transport, pending).await?;
                deferred = deferred.merge(d.deferred);
            }
            TransportEvent::Deadline => {
                // The handler has settled, so whether a 0x78 goes out no longer
                // reaches `settle`; only what the drains recorded is kept.
                let now = transport.now();
                let (reply_to, sid) = (serving.reply_to, serving.sid);
                let o = answer_overrun(session, transport, pending, now, reply_to, sid);
                deferred = deferred.merge(o.await?.deferred);
            }
            TransportEvent::Closed { .. } => return Ok(None),
            // As mid-handler: the 0x21 these owe is architecture open question 1.
            TransportEvent::DataInd { .. }
            | TransportEvent::DataTooLong { .. }
            | TransportEvent::Periodic { .. } => {}
        }
    }
}

/// What `answer_overrun` learned: whether a 0x78 was accepted for transmission, and what
/// its drains recorded.
#[derive(Debug, Clone, Copy)]
struct Overrun {
    /// The 0x78 was accepted by `s_data_req` (``UDSSVC_ARCH_0009`` rule 3).
    sent_pending: bool,
    /// What the tick's and the submission's drains recorded.
    deferred: Deferred,
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
    reply_to: Ai,
    sid: u8,
) -> Result<Overrun, T::Error> {
    let tick = session.tick(now);
    let first = drain(tick, transport, pending).await?;
    if !first.overran {
        return Ok(Overrun {
            sent_pending: false,
            deferred: first.deferred,
        });
    }
    let bytes = [0x7F_u8, sid, 0x78];
    let reaction = session.s_data_req(now, reply_to, &bytes, ServerTx::ResponsePending);
    let second = drain(reaction, transport, pending).await?;
    Ok(Overrun {
        sent_pending: second.rejected.is_none(),
        deferred: first.deferred.merge(second.deferred),
    })
}

#[cfg(test)]
mod tests {
    use super::Deferred;
    use uds_protocol::DiagnosticSessionType as S;

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
