//! The pump under a [`Client`]'s calls: one exchange at a time over `uds_session`'s
//! client role, its outputs drained into the transport.
//!
//! Every call goes through here. The exchange it runs is recorded in the client, not in
//! the call's future, so a call or a [`super::Responses`] dropped part-way leaves a
//! record the next call settles: a physical exchange is reset (``UDSS_LLR_0180``) and a
//! functional window is drained until it closes, its answers discarded.
//!
//! `Confirm` and `ResponseTimeout` name no channel, so they are matched to the exchange by
//! its [`Ai`]. That is exact here, because ``UDSS_LLR_0122`` makes an addressing unique
//! per channel and this driver runs one exchange at a time.
//!
//! `TransportEvent` has no start-of-message, so `t_data_som_ind` is never called and a
//! functional channel's responder table tracks only response-pending entries
//! (``UDSS_LLR_0137``, ``UDSS_LLR_0146``).

use super::encode::{self, Arrived, KEEP_ALIVE};
use super::{Client, ClientError, ClientKeepAlive, ClientSet};
use crate::storage::{ClientBuffers, ClientStorage};
use crate::{AfterSend, ClientTransport, TransportEvent};
use core::ops::Range;
use uds_session::{
    Address, Ai, Cause, ChannelAddressing, ChannelId, ChannelParams, ClientOutput,
    ClientReaction, ClientTx, ExpectedResponses, Finished, FunctionalChannelId, Mtype,
    PhysicalChannelId, Rejection, SResult, TaType, Timestamp, TransportError,
};

/// ISO 14229-2:2021 9.7 Table 9: a request is transmitted at most three times.
const REPEATS: u8 = 2;

/// What the client confirms to the session layer for a request the transport refused,
/// since no confirmation of it will ever come. Not a value any transport reports.
const REFUSED: SResult = SResult::Transport(TransportError(u16::MAX));

/// One request's progress, kept in the client so that it outlives the call that made it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Exchange {
    /// The request's addressing: its channel's, and what its confirmation and response
    /// timeout name.
    pub(super) ai: Ai,
    /// How much of the request buffer it occupies.
    pub(super) len: usize,
    /// Its service identifier, which a response answering it echoes.
    pub(super) sid: u8,
    /// The byte a positive response answering it carries after its service identifier,
    /// where the service echoes one.
    pub(super) echo: Option<u8>,
    /// How it was classified, `repeat` aside.
    pub(super) class: ClientTx,
    /// How many times it has been repeated.
    pub(super) repeats: u8,
    pub(super) phase: Phase,
}

/// Where an [`Exchange`] stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// Encoded, not yet accepted by the session layer.
    Unsent,
    /// Handed to the transport, its confirmation awaited.
    Sent,
    /// Confirmed sent; its response window is open.
    Open,
    /// A functional window whose expiry was seen with its last answer: the next step
    /// ends it.
    Closed,
}

impl Exchange {
    /// A request occupying `len` bytes of the request buffer, to `ai`.
    pub(super) const fn new(
        ai: Ai,
        len: usize,
        sid: u8,
        echo: Option<u8>,
        class: ClientTx,
    ) -> Self {
        Self {
            ai,
            len,
            sid,
            echo,
            class,
            repeats: 0,
            phase: Phase::Unsent,
        }
    }

    const fn functional(self) -> bool {
        matches!(self.ai.ta_type, TaType::Functional)
    }
}

/// A response that answered the exchange, as the place it occupies in the response
/// buffer.
#[derive(Debug, Clone)]
pub(super) struct Answered {
    /// The responding server's `S_AI[SA]`.
    pub(super) from: Address,
    pub(super) range: Range<usize>,
    pub(super) arrived: Arrived,
}

/// One open channel, as this driver books it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Open<Id> {
    ai: Ai,
    pub(super) id: Id,
    /// A transmission on it awaits its confirmation; withdrawing it would leave that
    /// confirmation to match a later channel with the same addressing (the open question
    /// "A late confirmation can match a reopened channel's request"), so it is never
    /// withdrawn meanwhile.
    unconfirmed: bool,
    /// Its server was put in a non-default session through this client.
    pub(super) in_session: bool,
    /// Its keep-alive fell due and has not been sent.
    owed: bool,
}

impl<Id> Open<Id> {
    const fn new(ai: Ai, id: Id) -> Self {
        Self {
            ai,
            id,
            unconfirmed: false,
            in_session: false,
            owed: false,
        }
    }

    const fn idle(&self) -> bool {
        !self.unconfirmed && !self.in_session && !self.owed
    }
}

/// The channels this driver has open, and the keep-alives it owes.
#[derive(Debug)]
pub(super) struct Book<const PHYS: usize, const FUNC: usize> {
    physical: [Option<Open<PhysicalChannelId>>; PHYS],
    functional: [Option<Open<FunctionalChannelId>>; FUNC],
    /// The client-wide keep-alive fell due and has not been sent (functional keep-alive).
    owed_functional: bool,
    /// The functional keep-alive awaiting its confirmation, and how often it has been
    /// repeated: ``UDSS_LLR_0157`` leaves the timer stopped after a failed one, and its
    /// repeat to the client.
    functional_keep_alive: Option<Ai>,
    functional_repeats: u8,
    /// A transmission the transport refused, still to be confirmed failed.
    refused: Option<Ai>,
}

impl<const PHYS: usize, const FUNC: usize> Book<PHYS, FUNC> {
    pub(super) const EMPTY: Self = Self {
        physical: [const { None }; PHYS],
        functional: [const { None }; FUNC],
        owed_functional: false,
        functional_keep_alive: None,
        functional_repeats: 0,
        refused: None,
    };

    pub(super) fn physical_to(
        &mut self,
        ta: Address,
    ) -> Option<&mut Open<PhysicalChannelId>> {
        self.physical.iter_mut().flatten().find(|o| o.ai.ta == ta)
    }

    fn functional_by(&self, ai: Ai) -> Option<FunctionalChannelId> {
        self.functional
            .iter()
            .flatten()
            .find(|o| o.ai == ai)
            .map(|o| o.id)
    }

    fn channel(&self, ai: Ai) -> Option<ChannelId> {
        match ai.ta_type {
            TaType::Physical => self
                .physical
                .iter()
                .flatten()
                .find(|o| o.ai == ai)
                .map(|o| o.id.into()),
            TaType::Functional => self.functional_by(ai).map(Into::into),
        }
    }

    /// Owe the functional keep-alive again after a failed confirmation, up to
    /// ISO 14229-2:2021 Table 9's repeats.
    fn owe_functional_repeat(&mut self) {
        if self.functional_repeats < REPEATS {
            self.functional_repeats = self.functional_repeats.saturating_add(1);
            self.owed_functional = true;
        }
    }

    /// Whether any channel's transmission awaits its confirmation.
    pub(super) fn unconfirmed(&self) -> bool {
        self.physical.iter().flatten().any(|o| o.unconfirmed)
            || self.functional.iter().flatten().any(|o| o.unconfirmed)
    }

    /// Mark every channel with this addressing as awaiting a confirmation or not.
    fn awaiting(&mut self, ai: Ai, unconfirmed: bool) {
        let physical = self.physical.iter_mut().flatten().filter(|o| o.ai == ai);
        for o in physical {
            o.unconfirmed = unconfirmed;
        }
        let functional = self.functional.iter_mut().flatten().filter(|o| o.ai == ai);
        for o in functional {
            o.unconfirmed = unconfirmed;
        }
    }
}

/// What one transport event meant for the exchange it was pumped for.
#[derive(Debug, Default)]
pub(super) struct Seen {
    /// The exchange's confirmation, and whether it reported success.
    pub(super) confirmed: Option<bool>,
    /// The exchange's response window expired.
    pub(super) timed_out: bool,
    /// A response answering the exchange.
    pub(super) answer: Option<Answered>,
    /// The connection the exchange ran over closed.
    pub(super) closed: bool,
}

/// What sending the owed keep-alives produced.
pub(super) struct Owed<E> {
    /// The running exchange's response window was found expired while sending, which the
    /// caller would otherwise never see again.
    pub(super) expired: bool,
    pub(super) result: Result<(), ClientError<E>>,
}

/// What draining one reaction produced.
struct Drained<X, E> {
    seen: Seen,
    outcome: Result<X, Rejection>,
    /// The transport's refusal of the reaction's transmission.
    refused: Option<E>,
}

/// Drain `reaction` into the transport and the book, and say what it meant for the
/// exchange addressed `exchange`.
///
/// Every `Transmit` reaches the transport here, the client always saying
/// [`AfterSend::Continue`]. One the transport refuses is booked for
/// [`Client::confirm_refused`], and the rest of the reaction is still drained, so that a
/// response timeout it reports is not lost. A `KeepAliveDue` is booked as owed rather than
/// answered, for [`Client::send_owed`] to send.
async fn drain<
    T: ClientTransport,
    K: ClientKeepAlive,
    X,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
>(
    mut reaction: ClientReaction<'_, '_, K, PHYS, FUNC, R, X>,
    transport: &mut T,
    book: &mut Book<PHYS, FUNC>,
    exchange: Option<Ai>,
) -> Drained<X, T::Error> {
    let mut found = Seen::default();
    let mut refused = None;
    for out in reaction.outputs() {
        match out {
            ClientOutput::Transmit { ai, data, .. } => {
                match transport.t_data_req(ai, data, AfterSend::Continue).await {
                    Ok(()) => book.awaiting(ai, true),
                    Err(error) => {
                        book.refused = Some(ai);
                        refused = Some(error);
                    }
                }
            }
            ClientOutput::Confirm { ai, result } => {
                book.awaiting(ai, false);
                let ok = result == SResult::Ok;
                if exchange == Some(ai) {
                    found.confirmed = Some(ok);
                } else if book.functional_keep_alive.take_if(|k| *k == ai).is_some() && !ok
                {
                    book.owe_functional_repeat();
                }
            }
            ClientOutput::ResponseTimeout { ai, .. } => {
                found.timed_out |= exchange == Some(ai);
            }
            ClientOutput::KeepAliveDue { channel: Some(id) } => {
                if let Some(o) = book.physical.iter_mut().flatten().find(|o| o.id == id) {
                    o.owed = true;
                }
            }
            ClientOutput::KeepAliveDue { channel: None } => {
                book.owed_functional = true;
                book.functional_repeats = 0;
            }
            // An indication's classification is the caller's, made before it was handed
            // in; a full responder table changes nothing here (UDSS_LLR_0143); and
            // whatever else UDSS_LLR_0012's open enumeration adds.
            _ => {}
        }
    }
    let Finished { outcome, rest: _ } = reaction.finish();
    Drained {
        seen: found,
        outcome,
        refused,
    }
}

/// The earlier of two optional deadlines on the wrapping timebase.
fn earliest(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.has_reached(a) { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Where `data` sits in the buffer whose first byte is at address `base`.
fn locate(base: usize, data: &[u8]) -> Range<usize> {
    let start = data.as_ptr().addr().saturating_sub(base);
    start..start.saturating_add(data.len())
}

impl<
    C: ClientSet,
    T: ClientTransport,
    K: ClientKeepAlive,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> Client<C, T, K, PHYS, FUNC, R>
{
    /// The addressing of this client's channel to `ta`, of either kind.
    pub(super) const fn addressing(&self, ta: Address) -> ChannelAddressing {
        ChannelAddressing {
            mtype: Mtype::Diag,
            sa: self.tester,
            ta,
        }
    }

    /// The physical channel to `target`, opened if it is not yet.
    ///
    /// Where every slot is taken, an idle one is withdrawn to make room
    /// (``UDSS_LLR_0125``): one with no confirmation outstanding, no session entered and
    /// no keep-alive owed.
    pub(super) fn physical_channel(
        &mut self,
        target: Address,
    ) -> Result<Ai, ClientError<T::Error>> {
        if let Some(o) = self.book.physical_to(target) {
            return Ok(o.ai);
        }
        let now = self.transport.now();
        if self.book.physical.iter().all(Option::is_some) {
            let idle = self
                .book
                .physical
                .iter_mut()
                .find(|s| s.is_some_and(|o| o.idle()));
            let Some(slot) = idle else {
                return Err(ClientError::NoChannel);
            };
            if let Some(o) = slot.take() {
                let _expiries_stay_in_the_session =
                    self.session.withdraw_channel(now, o.id).finish();
            }
        }
        let params = ChannelParams {
            reloads: self.transport.channel_timing(),
            spacing: self.timing.physical_spacing,
        };
        let addressing = self.addressing(target);
        let id =
            K::open_physical(&mut self.session, now, addressing, params, self.keep_alive)
                .map_err(|_| ClientError::NoChannel)?;
        let ai = addressing.with_ta_type(TaType::Physical);
        let free = self.book.physical.iter_mut().find(|s| s.is_none());
        if let Some(slot) = free {
            *slot = Some(Open::new(ai, id));
        }
        Ok(ai)
    }

    /// The functional channel to `group`, opened if it is not yet.
    ///
    /// The keep-alive's own group is never withdrawn (the open question "Functional
    /// keep-alive falls due with no functional channel open").
    pub(super) fn functional_channel(
        &mut self,
        group: Address,
    ) -> Result<Ai, ClientError<T::Error>> {
        let ai = self.addressing(group).with_ta_type(TaType::Functional);
        if self.book.functional_by(ai).is_some() {
            return Ok(ai);
        }
        let now = self.transport.now();
        let keep = K::group(self.keep_alive);
        if self.book.functional.iter().all(Option::is_some) {
            let idle = self
                .book
                .functional
                .iter_mut()
                .find(|s| s.is_some_and(|o| !o.unconfirmed && Some(o.ai.ta) != keep));
            let Some(slot) = idle else {
                return Err(ClientError::NoChannel);
            };
            if let Some(o) = slot.take() {
                let _expiries_stay_in_the_session =
                    self.session.withdraw_channel(now, o.id).finish();
            }
        }
        let params = ChannelParams {
            reloads: self.transport.channel_timing(),
            spacing: self.timing.functional_spacing,
        };
        let Finished { outcome, rest: _ } = self
            .session
            .open_functional_channel(now, self.addressing(group), params)
            .finish();
        let id = outcome.map_err(|_| ClientError::NoChannel)?;
        let free = self.book.functional.iter_mut().find(|s| s.is_none());
        if let Some(slot) = free {
            *slot = Some(Open::new(ai, id));
        }
        Ok(ai)
    }

    /// Reset the channel `ai` names, ending whatever it had in progress
    /// (``UDSS_LLR_0180``).
    pub(super) fn abandon(&mut self, ai: Ai) {
        if let Some(channel) = self.book.channel(ai) {
            let now = self.transport.now();
            let _expiries_stay_in_the_session =
                self.session.reset_channel(now, channel).finish();
        }
    }

    /// Confirm failed, to the session layer, a transmission the transport refused: its
    /// association would otherwise stay outstanding for good (``UDSS_LLR_0060``).
    fn confirm_refused(&mut self) {
        if let Some(ai) = self.book.refused.take() {
            let now = self.transport.now();
            let _expiries_stay_in_the_session =
                self.session.t_data_conf(now, ai, REFUSED).finish();
        }
    }

    /// Settle what a dropped call or [`super::Responses`] left: a request never sent is
    /// forgotten, a functional window still open is set aside to drain, and anything
    /// else is reset.
    pub(super) fn retire(&mut self) {
        let Some(e) = self.exchange.take() else {
            return;
        };
        match e.phase {
            Phase::Unsent | Phase::Closed => {}
            Phase::Open if e.functional() => {
                if let Some(older) = self.draining.replace(e) {
                    self.abandon(older.ai);
                }
            }
            Phase::Sent | Phase::Open => self.abandon(e.ai),
        }
    }

    /// Wait out a functional window set aside by [`Self::retire`], discarding its answers,
    /// so none of them is taken for the next exchange's.
    pub(super) async fn drain_window(&mut self) -> Result<(), ClientError<T::Error>> {
        while let Some(e) = self.draining {
            let seen = match self.pump(Some(e), None).await {
                Ok(seen) => seen,
                Err(error) => {
                    self.draining = None;
                    return Err(error);
                }
            };
            if seen.timed_out {
                self.draining = None;
            }
        }
        Ok(())
    }

    /// Hand the exchange's request to the session layer, waiting out a spacing timer
    /// (``UDSS_LLR_0171``) or an earlier transmission's confirmation (``UDSS_LLR_0061``)
    /// on the way.
    async fn submit(&mut self, mut e: Exchange) -> Result<Exchange, ClientError<T::Error>> {
        let class = match e.class {
            ClientTx::Request {
                expected, session, ..
            } => ClientTx::Request {
                expected,
                repeat: e.repeats > 0,
                session,
            },
            keep_alive @ ClientTx::KeepAlive { .. } => keep_alive,
        };
        loop {
            let now = self.transport.now();
            let ClientBuffers { request, .. } = self.store.split();
            let data = request.get(..e.len).unwrap_or(&[]);
            let reaction = self.session.s_data_req(now, e.ai, data, class);
            // Sent while the transport takes it, so a call dropped meanwhile is reset.
            self.exchange = Some(Exchange {
                phase: Phase::Sent,
                ..e
            });
            let drained = drain(reaction, &mut self.transport, &mut self.book, None).await;
            self.exchange = Some(e);
            self.confirm_refused();
            if let Some(error) = drained.refused {
                return Err(ClientError::Transport(error));
            }
            match drained.outcome {
                Ok(()) => {
                    // The request keeps its own server alive (UDSS_LLR_0160).
                    if let Some(o) =
                        self.book.physical_to(e.ai.ta).filter(|_| !e.functional())
                    {
                        o.owed = false;
                    }
                    e.phase = Phase::Sent;
                    self.exchange = Some(e);
                    return Ok(e);
                }
                Err(r)
                    if r.contains(Cause::SpacingTimerRunning)
                        || r.contains(Cause::AssociationOutstanding) =>
                {
                    // An unsent request is answered by nothing, so what arrives meanwhile
                    // is only drained.
                    self.pump(Some(e), None).await?;
                }
                Err(_) => return Err(ClientError::NotSent),
            }
        }
    }

    /// Take the exchange one answer further: the next response that answers it, or
    /// `None` once a functional window has closed.
    ///
    /// Sends the request first if it is unsent. A failed transmission and, physically, an
    /// expired response window are repeated up to ISO 14229-2:2021 Table 9's two times;
    /// after the last, the channel is reset and the failure returned. A physical exchange
    /// ends at its answer, a functional one at its window closing.
    pub(super) async fn advance(
        &mut self,
    ) -> Option<Result<Answered, ClientError<T::Error>>> {
        let mut e = self.exchange?;
        loop {
            match e.phase {
                Phase::Closed => {
                    self.exchange = None;
                    return self.send_owed().await.result.err().map(Err);
                }
                Phase::Unsent => {
                    let opened = match self.book.channel(e.ai) {
                        Some(_) => Ok(e.ai),
                        None if e.functional() => self.functional_channel(e.ai.ta),
                        None => self.physical_channel(e.ai.ta),
                    };
                    let submitted = match opened {
                        Ok(_) => self.submit(e).await,
                        Err(error) => Err(error),
                    };
                    e = match submitted {
                        Ok(e) => e,
                        Err(error) => return Some(self.fail(e, error)),
                    };
                }
                Phase::Sent | Phase::Open => {}
            }
            let mut seen = match self.pump(Some(e), None).await {
                Ok(seen) => seen,
                Err(error) => return Some(self.fail(e, error)),
            };
            // A keep-alive the transport fails to take stays owed; the exchange goes on.
            seen.timed_out |= self.send_owed().await.expired;
            if seen.confirmed == Some(true) {
                e.phase = Phase::Open;
            }
            self.exchange = Some(e);
            if let Some(answered) = seen.answer {
                if !e.functional() {
                    self.exchange = None;
                } else if seen.timed_out {
                    e.phase = Phase::Closed;
                    self.exchange = Some(e);
                }
                return Some(Ok(answered));
            }
            if seen.closed {
                return Some(self.fail(e, ClientError::Closed));
            }
            let failure = if seen.confirmed == Some(false) {
                ClientError::NotSent
            } else if seen.timed_out && e.functional() {
                e.phase = Phase::Closed;
                self.exchange = Some(e);
                continue;
            } else if seen.timed_out {
                ClientError::Timeout
            } else {
                continue;
            };
            if e.repeats >= REPEATS {
                return Some(self.fail(e, failure));
            }
            e.repeats = e.repeats.saturating_add(1);
            e.phase = Phase::Unsent;
            self.exchange = Some(e);
        }
    }

    /// End the exchange with `error`, resetting its channel.
    fn fail(
        &mut self,
        e: Exchange,
        error: ClientError<T::Error>,
    ) -> Result<Answered, ClientError<T::Error>> {
        self.abandon(e.ai);
        self.exchange = None;
        Err(error)
    }

    /// Send every keep-alive that has fallen due and that may go now. One the session
    /// layer refuses for now stays owed, and so does one the transport refuses, which
    /// ends the sending.
    ///
    /// None goes to a server a request is awaiting. For a physical exchange that is only
    /// its own server, whose keep-alive its request stopped (``UDSS_LLR_0160``); a
    /// functional window awaits every server, so none goes while one is open.
    pub(super) async fn send_owed(&mut self) -> Owed<T::Error> {
        let busy = self.exchange.map(|e| e.ai);
        let window = self.exchange.is_some_and(Exchange::functional);
        let mut expired = false;
        let result = self.send_owed_noting(busy, window, &mut expired).await;
        Owed { expired, result }
    }

    async fn send_owed_noting(
        &mut self,
        busy: Option<Ai>,
        window: bool,
        expired: &mut bool,
    ) -> Result<(), ClientError<T::Error>> {
        for i in 0..PHYS {
            let owed = self
                .book
                .physical
                .get(i)
                .copied()
                .flatten()
                .filter(|o| o.owed && !window && Some(o.ai) != busy);
            let Some(o) = owed else {
                continue;
            };
            let accepted = self.keep_alive_on(o.ai, expired).await?;
            if accepted && let Some(Some(o)) = self.book.physical.get_mut(i) {
                o.owed = false;
            }
        }
        if self.book.owed_functional
            && !window
            && let Some(group) = K::group(self.keep_alive)
            && let Ok(ai) = self.functional_channel(group)
            && self.keep_alive_on(ai, expired).await?
        {
            self.book.owed_functional = false;
            self.book.functional_keep_alive = Some(ai);
        }
        Ok(())
    }

    /// End every keep-alive, as the session ends (``UDSS_LLR_0184``).
    pub(super) fn end_keep_alives(&mut self) {
        let now = self.transport.now();
        for o in self.book.physical.iter_mut().flatten() {
            let _expiries_stay_in_the_session =
                self.session.release_keep_alive(now, o.id).finish();
            o.in_session = false;
            o.owed = false;
        }
        self.settle_functional_keep_alive();
    }

    /// Release the functional keep-alive once no server this client put in a
    /// non-default session is still in one (``UDSS_LLR_0184``). It reaches every server,
    /// so only the last one leaving ends it; ``UDSS_LLR_0158`` would read only a
    /// functionally addressed return to the default session.
    pub(super) fn settle_functional_keep_alive(&mut self) {
        let Some(group) = K::group(self.keep_alive) else {
            return;
        };
        if self.book.physical.iter().flatten().any(|o| o.in_session) {
            return;
        }
        let Some(id) = self
            .functional_channel(group)
            .ok()
            .and_then(|ai| self.book.functional_by(ai))
        else {
            return;
        };
        let now = self.transport.now();
        let _expiries_stay_in_the_session =
            self.session.release_keep_alive(now, id).finish();
        self.book.owed_functional = false;
        self.book.functional_repeats = 0;
    }

    /// Send the keep-alive `TesterPresent` on the channel `ai` names, and say whether the
    /// session layer accepted it. Where the running exchange's response window was found
    /// expired on the way, `expired` is set, since the input that finds it reports it
    /// once.
    async fn keep_alive_on(
        &mut self,
        ai: Ai,
        expired: &mut bool,
    ) -> Result<bool, ClientError<T::Error>> {
        let now = self.transport.now();
        let class = ClientTx::KeepAlive {
            expected: ExpectedResponses::None,
        };
        let running = self.exchange.map(|e| e.ai);
        let reaction = self.session.s_data_req(now, ai, &KEEP_ALIVE, class);
        let drained = drain(reaction, &mut self.transport, &mut self.book, running).await;
        self.confirm_refused();
        *expired |= drained.seen.timed_out;
        match drained.refused {
            Some(error) => Err(ClientError::Transport(error)),
            None => Ok(drained.outcome.is_ok()),
        }
    }

    /// Wait for one transport event and hand it to the session layer, reporting what it
    /// meant for `subject`. `until` bounds the wait beside the session's own deadline.
    ///
    /// ``UDSS_LLR_0026`` — the caller identifies an indication's channel. A response is
    /// the functional window's while one is open, and otherwise the physical channel to
    /// its sender's; one from a server no channel names is not indicated at all.
    /// It answers `subject` only where it echoes its service; see [`encode::classify`].
    ///
    /// A close releases the keep-alive of every channel to that peer (``UDSS_LLR_0184``):
    /// the server's session went with the connection, so a `TesterPresent` would keep
    /// nothing alive, and the functional keep-alive goes once no server is left in session,
    /// unless a session change is running, which settles it when it ends. A close ends a
    /// physical exchange with that peer, but not a functional window, which awaits every
    /// server and closes at its own timeout.
    pub(super) async fn pump(
        &mut self,
        subject: Option<Exchange>,
        until: Option<Timestamp>,
    ) -> Result<Seen, ClientError<T::Error>> {
        let subject = subject.filter(|e| e.phase != Phase::Unsent);
        let deadline = earliest(self.session.next_deadline(), until);
        let ClientBuffers { response, .. } = self.store.split();
        let base = response.as_ptr().addr();
        let event = self
            .transport
            .next_event(response, deadline)
            .await
            .map_err(ClientError::Transport)?;
        let now = self.transport.now();
        let mut answer = None;
        let mut closed = false;
        let mut left = false;
        let inbound = match event {
            TransportEvent::DataInd { ai, data } => Ok((ai, data, Arrived::Whole)),
            TransportEvent::DataTooLong { ai, data, declared } => {
                Ok((ai, data, Arrived::TooLong(declared)))
            }
            other => Err(other),
        };
        let reaction = match inbound {
            Ok((ai, data, arrived)) => {
                let Some((channel, answering)) =
                    route(&self.book, self.tester, subject, ai)
                else {
                    return Ok(Seen::default());
                };
                let selection = answering.and_then(|e| e.class.session_selection());
                let expected = answering.map(|e| (e.sid, e.echo));
                let class = encode::classify(expected, data, selection);
                if answering.is_some() && encode::solicited_final(class) {
                    answer = Some(Answered {
                        from: ai.sa,
                        range: locate(base, data),
                        arrived,
                    });
                }
                self.session
                    .t_data_ind(now, channel, ai, data, SResult::Ok, Some(class))
            }
            Err(TransportEvent::DataConf { ai, result }) => {
                self.session.t_data_conf(now, ai, result)
            }
            Err(TransportEvent::Closed { peer, .. }) => {
                for o in self
                    .book
                    .physical
                    .iter_mut()
                    .flatten()
                    .filter(|o| o.ai.ta == peer)
                {
                    let _ = self.session.release_keep_alive(now, o.id).finish();
                    left |= o.in_session;
                    o.in_session = false;
                    o.owed = false;
                }
                closed = subject.is_some_and(|e| !e.functional() && e.ai.ta == peer);
                self.session.tick(now)
            }
            Err(_) => self.session.tick(now),
        };
        let drained = drain(
            reaction,
            &mut self.transport,
            &mut self.book,
            subject.map(|e| e.ai),
        )
        .await;
        self.confirm_refused();
        let changing = self
            .exchange
            .is_some_and(|e| e.class.session_selection().is_some());
        if left && !changing {
            self.settle_functional_keep_alive();
        }
        if let Some(error) = drained.refused {
            return Err(ClientError::Transport(error));
        }
        Ok(Seen {
            answer,
            closed,
            ..drained.seen
        })
    }
}

/// The channel a message from a server belongs to, and the exchange it may answer: the
/// functional window's while one is open, else the physical channel to its sender's. Only
/// a confirmed request is answered: the session layer opens its window at the
/// confirmation, so a response before it would leave that window open behind it.
/// `None` for a message to another tester, or from a server no channel names.
fn route<const PHYS: usize, const FUNC: usize>(
    book: &Book<PHYS, FUNC>,
    tester: Address,
    subject: Option<Exchange>,
    ai: Ai,
) -> Option<(ChannelId, Option<Exchange>)> {
    if ai.ta != tester {
        return None;
    }
    let open = subject.filter(|e| e.phase == Phase::Open);
    if let Some(e) = subject.filter(|e| e.functional()) {
        return Some((book.functional_by(e.ai)?.into(), open));
    }
    let o = book.physical.iter().flatten().find(|o| o.ai.ta == ai.sa)?;
    Some((o.id.into(), open.filter(|e| e.ai.ta == ai.sa)))
}
