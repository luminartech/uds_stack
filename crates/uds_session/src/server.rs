//! The server role.
//!
//! ``UDSS_LLR_0029`` fixes an instance's role at creation and observes that "a node that
//! is both, a gateway or a tester under test, is two instances". This crate makes that
//! literal: [`Server`] and [`crate::Client`] are separate types, so the cross-role inputs
//! ``UDSS_LLR_0030`` lists are unrepresentable rather than rejected — the discharge
//! ``UDSS_LLR_0027`` describes when it says an interface in which an identifier cannot be
//! omitted satisfies the requirement without a check.

use crate::addressing::{Address, AddressExtension, Ai, PeerIdentity};
use crate::classification::{ServerRx, ServerTx, SessionSelection, Solicitation};
use crate::params::{ServerParameter, ServerParams, ServerReload};
use crate::reaction::Reaction;
use crate::rejection::{Cause, Rejection};
use crate::result::SResult;
use crate::time::{Timestamp, earlier};
use crate::timer::{Reaches, Timer};

/// One transmission between its `S_Data.req` and its `T_Data.conf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outstanding {
    ai: Ai,
    class: ServerTx,
}

/// One slot of the association storage ``UDSS_LLR_0059`` requires.
///
/// The association binds an `S_Data.req`'s classification and addressing to the
/// `T_Data.req` produced from it and the `T_Data.conf` that reports its outcome, from the
/// request until the confirmation. ``UDSS_LLR_0060`` permits at most one outstanding per
/// addressing; ``UDSS_LLR_0062`` rejects a request for which none is free, so the array's
/// length is the server's capacity.
///
/// Storage is moved into the instance, never duplicated: a copy of an outstanding
/// association is an association the session layer does not know it has, which is why
/// `Copy` and equality are absent and the array is built from [`Association::EMPTY`].
#[derive(Debug)]
pub struct Association {
    slot: Option<Outstanding>,
}

impl Association {
    /// A free slot.
    ///
    /// ``UDSS_LLR_0064`` — no association is outstanding on initialisation.
    pub const EMPTY: Self = Self { slot: None };
}

/// ``UDSS_LLR_0082`` — the session fact, and while non-default the controlling client and
/// the one `tS3_Server`. The timer lives here because ``UDSS_LLR_0099`` disables it in the
/// default session: there is no timer to hold while `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    Default,
    NonDefault {
        client: PeerIdentity,
        s3: Timer<Reaches>,
    },
}

impl Session {
    /// Whether `tS3_Server` is running; `None` in the default session, which has no
    /// timer (``UDSS_LLR_0099``).
    const fn s3_running(self) -> Option<bool> {
        match self {
            Self::NonDefault { s3, .. } => Some(s3.is_running()),
            Self::Default => None,
        }
    }
}

/// ``UDSS_LLR_0104`` — the service in progress, its response-pending anchor, and
/// ``UDSS_LLR_0101``'s `tP2_Server` with the parameter it carries and the lead of
/// ``UDSS_LLR_0186`` taken with it. The timer lives here because ``UDSS_LLR_0113``–``0117``
/// only ever run it for a service in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InProgress {
    peer: PeerIdentity,
    anchor: Option<Timestamp>,
    p2: Timer<Reaches>,
    loaded: ServerReload,
    lead: u32,
    /// The transmission this service submitted that is still unconfirmed: the only one
    /// whose `T_Data.conf` answers it (``UDSS_LLR_0109``). `None` for a service that
    /// replaced another (``UDSS_LLR_0108``), so its predecessor's confirmation does not.
    outstanding: Option<Outstanding>,
}

/// What a server produces for the caller to retrieve.
///
/// ``UDSS_LLR_0012`` enumerates the standard's own outputs and leaves the enumeration
/// open, so that outputs the standard does not define reach the application by the same
/// mechanism. That requirement is what `#[non_exhaustive]` here rests on: it is one of
/// only two in the set — ``UDSS_LLR_0010`` for inputs is the other — that state an open
/// enumeration, and everywhere else the requirement set closes its own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerOutput<'d> {
    /// `T_Data.req` — ``UDSS_LLR_0024``. The data is the caller's own, per
    /// ``UDSS_LLR_0014``.
    Transmit {
        /// Where it goes.
        ai: Ai,
        /// What to send.
        data: &'d [u8],
    },
    /// `S_Data.ind` — ``UDSS_LLR_0034`` and ``UDSS_LLR_0036``. ``UDSS_LLR_0035`` makes
    /// `data` meaningful only where `result` is [`SResult::Ok`].
    Indicate {
        /// Who it came from and who it was for.
        ai: Ai,
        /// The message.
        data: &'d [u8],
        /// The outcome of the reception.
        result: SResult,
    },
    /// `S_Data.conf` — ``UDSS_LLR_0037``, confirming a preceding `S_Data.req`.
    Confirm {
        /// The addressing that identifies the request being confirmed, per
        /// ``UDSS_LLR_0059``.
        ai: Ai,
        /// The outcome of the transmission.
        result: SResult,
    },
    /// The non-default session ended because `tS3_Server` expired.
    ///
    /// ``UDSS_LLR_0100`` — not a primitive the standard defines. ISO 14229-2 specifies
    /// the timer but leaves the resulting transition to the application layer, where
    /// ISO 14229-1:2020 10.2.2.2 Table 25 states it.
    SessionTimeout {
        /// The controlling client whose session ended, per ``UDSS_LLR_0082`` and
        /// ``UDSS_LLR_0044``.
        client: PeerIdentity,
    },
    /// `tP2_Server` is about to expire with no response transmitted.
    ///
    /// ``UDSS_LLR_0117`` — an overrun the session layer can observe and cannot correct,
    /// so it reports and the application acts. ISO 14229-2 states no session layer action.
    /// It is delivered the response-pending lead of ``UDSS_LLR_0186`` before the window
    /// closes, so that a response-pending message sent on it goes out within the window;
    /// with a lead of zero, at the close.
    ResponseOverrun {
        /// The `S_AI[SA]` of the service in progress, per ``UDSS_LLR_0104``.
        sa: Address,
        /// Its `S_AI[AE]`, where `S_Mtype` carries one.
        ae: Option<AddressExtension>,
        /// Which of `tP2_Server_Max` and `tP2*_Server_Max` the timer was carrying.
        loaded: ServerReload,
    },
}

/// A reaction carrying server outputs.
pub type ServerReaction<'s, 'd, const A: usize, T = ()> =
    Reaction<'s, 'd, ServerOutput<'d>, Server<A>, T>;

/// The session layer in the server role.
///
/// ``UDSS_LLR_0008`` — all state lives here, in the caller-supplied storage this owns;
/// nothing is retained anywhere else between inputs.
#[derive(Debug)]
pub struct Server<const A: usize> {
    associations: [Association; A],
    params: ServerParams,
    session: Session,
    service: Option<InProgress>,
    /// Expiry snapshots: taken at the instant of expiry, because what the indication
    /// names may be gone by the time it is drained. ``UDSS_LLR_0100``'s expiry discards
    /// the controlling client its indication carries; ``UDSS_LLR_0117``'s names the
    /// service in progress, which the very input that ran the expiry may replace under
    /// ``UDSS_LLR_0108``. So each is a snapshot of the indication's facts, not a flag
    /// that it is owed.
    s3_expiry: Option<PeerIdentity>,
    p2_expiry: Option<(PeerIdentity, ServerReload)>,
}

impl<const A: usize> Server<A> {
    /// Create a server.
    ///
    /// ``UDSS_LLR_0032`` — creation supplies the association storage of
    /// ``UDSS_LLR_0059`` and the `tS3_Server`, `tP2_Server_Max` and `tP2*_Server_Max`
    /// parameters of ``UDSS_LLR_0042``, which have no defaults, with the response-pending
    /// lead of ``UDSS_LLR_0186``. The lead's bounds are not checked here, so that this
    /// stays infallible; [`ServerParams::is_well_formed`] states them. The storage is
    /// supplied by value: ``UDSS_LLR_0004`` forbids allocation and the number of peers is
    /// a property of the deployment, so the caller sizes it as `A` and hands it over.
    /// ``UDSS_LLR_0008`` is satisfied in both its branches at once — the caller supplies
    /// the storage, and it then lives in the instance. `A` is the capacity
    /// ``UDSS_LLR_0062`` rejects against.
    ///
    /// `A` is at least one, checked at compile time: with no association every
    /// `s_data_req` is refused ``UDSS_LLR_0062``'s way, so the server could never answer.
    ///
    /// ```compile_fail,E0080
    /// use uds_session::{Server, ServerParams};
    ///
    /// let params = ServerParams {
    ///     s3_server: 5_000,
    ///     p2_server_max: 50,
    ///     p2_star_server_max: 5_000,
    ///     response_pending_lead: 0,
    /// };
    /// let _mute = Server::<0>::new([], params);
    /// ```
    #[must_use]
    pub const fn new(associations: [Association; A], params: ServerParams) -> Self {
        const { assert!(A >= 1, "a server needs at least one association") };
        Self {
            associations,
            params,
            session: Session::Default,
            service: None,
            s3_expiry: None,
            p2_expiry: None,
        }
    }

    /// ``UDSS_LLR_0081`` — act on every expiry the timestamp causes before the input.
    /// A snapshot stays until it is retrieved (``UDSS_LLR_0011``); a later expiry of the
    /// same timer replaces one never retrieved. `tP2_Server` is read the service's lead
    /// ahead of its expiry (``UDSS_LLR_0117``, ``UDSS_LLR_0186``).
    fn expire(&mut self, now: Timestamp) {
        if let Session::NonDefault { client, s3 } = self.session
            && s3.expired(now)
        {
            // UDSS_LLR_0100
            self.s3_expiry = Some(client);
            self.session = Session::Default;
        }
        if let Some(service) = self.service.as_mut()
            && service.p2.expired_by(now, service.lead)
        {
            // UDSS_LLR_0117 — stop, and report the service and the parameter; 0186 is
            // the lead, the timer itself still loaded with the whole window.
            self.p2_expiry = Some((service.peer, service.loaded));
            service.p2.stop();
        }
    }

    /// ``UDSS_LLR_0082`` — whether `peer` is the controlling client.
    fn is_controlling(&self, peer: PeerIdentity) -> bool {
        matches!(self.session, Session::NonDefault { client, .. } if client == peer)
    }

    /// Start, or restart, `tS3_Server` — a no-op in the default session
    /// (``UDSS_LLR_0099``), which the type makes the only possible outcome.
    fn restart_s3(&mut self, now: Timestamp) {
        if let Session::NonDefault { s3, .. } = &mut self.session {
            s3.start(now, self.params.s3_server);
        }
    }

    /// Stop `tS3_Server` — likewise a no-op in the default session.
    fn stop_s3(&mut self) {
        if let Session::NonDefault { s3, .. } = &mut self.session {
            s3.stop();
        }
    }

    /// Enter a non-default session controlled by `client`, `tS3_Server` started
    /// (``UDSS_LLR_0085``, ``UDSS_LLR_0086``).
    fn enter_non_default(&mut self, now: Timestamp, client: PeerIdentity) {
        let mut s3 = Timer::STOPPED;
        s3.start(now, self.params.s3_server);
        self.session = Session::NonDefault { client, s3 };
    }

    /// ``UDSS_LLR_0106`` — whether an input addressing `peer` answers the service in
    /// progress: a submission to it, or a completion report from it.
    fn serves(&self, peer: PeerIdentity) -> bool {
        self.service.is_some_and(|s| s.peer == peer)
    }

    /// ``UDSS_LLR_0109`` — whether the confirmation of `sent` answers the service in
    /// progress: only where that service submitted it. Addressing alone would match a
    /// replaced service's response (``UDSS_LLR_0108``) to the one that replaced it.
    fn answers(&self, sent: Outstanding) -> bool {
        self.service.is_some_and(|s| s.outstanding == Some(sent))
    }

    /// ``UDSS_LLR_0119`` — ⌈3 × `tP2*_Server_Max` / 10⌉ in integer arithmetic.
    const fn spacing(&self) -> u32 {
        self.params.response_pending_spacing()
    }

    /// Every cause ``UDSS_LLR_0016`` requires the report to state, over `&self`.
    ///
    /// Validation borrows the server shared, so it cannot change state, and a rejected
    /// input leaves the state as the expiries left it (``UDSS_LLR_0015``): the borrow
    /// checker holds it, not a review.
    fn validate_req(
        &self,
        now: Timestamp,
        ai: Ai,
        class: ServerTx,
    ) -> Result<(), Rejection> {
        let mut causes: Option<Rejection> = None;
        let mut add = |c: Cause| {
            causes = Some(causes.map_or(Rejection::new(c), |r| r.with(c)));
        };
        let mut outstanding = self.associations.iter().filter_map(|a| a.slot);
        if outstanding.clone().any(|o| o.ai == ai) {
            add(Cause::AssociationOutstanding); // UDSS_LLR_0061
        }
        if self.associations.iter().all(|a| a.slot.is_some()) {
            add(Cause::NoAssociationFree); // UDSS_LLR_0062
        }
        if class == ServerTx::ResponsePending && self.serves(ai.target()) {
            let unconfirmed = outstanding.any(|o| {
                o.class == ServerTx::ResponsePending && o.ai.target() == ai.target()
            });
            if unconfirmed {
                add(Cause::ResponsePendingUnconfirmed); // UDSS_LLR_0118
            }
            let too_soon = self
                .service
                .and_then(|s| s.anchor)
                .is_some_and(|t| now.interval_since(t) < self.spacing());
            if too_soon {
                add(Cause::ResponsePendingTooSoon); // UDSS_LLR_0119
            }
        }
        causes.map_or(Ok(()), Err)
    }

    /// Set a protocol parameter.
    ///
    /// ``UDSS_LLR_0040`` puts this in the service interface; ``UDSS_LLR_0043`` permits it
    /// at any time. ``UDSS_LLR_0076`` leaves a running timer on the value it was loaded
    /// with, so a change never moves a window already open. ``UDSS_LLR_0010`` makes this
    /// an act of the caller rather than a primitive, which is why it takes a timestamp
    /// and why ``UDSS_LLR_0081`` orders that timestamp's expiries ahead of it.
    pub fn set_parameter(
        &mut self,
        now: Timestamp,
        parameter: ServerParameter,
    ) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        match parameter {
            ServerParameter::S3Server(v) => self.params.s3_server = v,
            ServerParameter::P2ServerMax(v) => self.params.p2_server_max = v,
            ServerParameter::P2StarServerMax(v) => self.params.p2_star_server_max = v,
            ServerParameter::ResponsePendingLead(v) => {
                self.params.response_pending_lead = v;
            }
        }
        Reaction::new(self, [None, None], Ok(()))
    }

    /// Request transmission of a response.
    ///
    /// ``UDSS_LLR_0033``. ``UDSS_LLR_0054`` rejects a length differing from the data
    /// supplied, which passing a slice discharges. ``UDSS_LLR_0061`` and
    /// ``UDSS_LLR_0062`` reject a request whose addressing already has a transmission
    /// outstanding or for which no association is free; ``UDSS_LLR_0118`` and
    /// ``UDSS_LLR_0119`` reject a response-pending message that is unconfirmed or too
    /// soon. One report states every cause that held (``UDSS_LLR_0016``). Otherwise the
    /// request takes an association (``UDSS_LLR_0059``), stops `tP2_Server` where it
    /// answers the service in progress (``UDSS_LLR_0114``), being then the transmission
    /// whose confirmation answers that service (``UDSS_LLR_0109``), and is passed on as a
    /// `T_Data.req`. A [`ServerTx::BusyRepeatRequest`] takes an association and answers no
    /// service (``UDSS_LLR_0187``).
    pub fn s_data_req<'d>(
        &mut self,
        now: Timestamp,
        ai: Ai,
        data: &'d [u8],
        class: ServerTx,
    ) -> ServerReaction<'_, 'd, A> {
        self.expire(now);
        if let Err(rejection) = self.validate_req(now, ai, class) {
            return Reaction::new(self, [None, None], Err(rejection)); // UDSS_LLR_0015
        }
        // UDSS_LLR_0059 — take a free association.
        let sent = Outstanding { ai, class };
        if let Some(free) = self.associations.iter_mut().find(|a| a.slot.is_none()) {
            free.slot = Some(sent);
        }
        // UDSS_LLR_0114
        let stops_p2 = match class {
            ServerTx::ResponsePending => true,
            ServerTx::FinalResponse { solicitation, .. } => {
                solicitation == Solicitation::Solicited
            }
            ServerTx::BusyRepeatRequest => false, // UDSS_LLR_0187
        };
        if stops_p2
            && self.serves(ai.target())
            && let Some(service) = self.service.as_mut()
        {
            service.p2.stop();
            service.outstanding = Some(sent); // UDSS_LLR_0109: its conf answers this one
        }
        Reaction::new(
            self,
            [Some(ServerOutput::Transmit { ai, data }), None],
            Ok(()),
        )
    }

    /// A message has started arriving.
    ///
    /// ``UDSS_LLR_0023`` — addressing and no data, length or result.
    /// ``UDSS_LLR_0038`` keeps it inside the session layer; it is never forwarded, and
    /// the reaction carries no output of its own. A request not marked keep-alive from
    /// the controlling client stops `tS3_Server` (``UDSS_LLR_0087``); one marked
    /// keep-alive changes nothing (``UDSS_LLR_0096``). It begins no service in progress
    /// (``UDSS_LLR_0107``), and the server keeps no start-of-message state of its own:
    /// ``UDSS_LLR_0045`` gives that to the client alone. ``UDSS_LLR_0030`` bars a server
    /// from receiving a response, which [`ServerRx`] cannot express, and from an
    /// indication naming a channel, which this signature has no parameter for.
    pub fn t_data_som_ind(
        &mut self,
        now: Timestamp,
        ai: Ai,
        class: ServerRx,
    ) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        // UDSS_LLR_0087; a keep-alive start-of-message changes nothing (UDSS_LLR_0096).
        // The server keeps no start-of-message state: UDSS_LLR_0045 gives that to the
        // client alone.
        if matches!(class, ServerRx::Request { .. }) && self.is_controlling(ai.source()) {
            self.stop_s3();
        }
        Reaction::new(self, [None, None], Ok(())) // UDSS_LLR_0038 — never forwarded
    }

    /// A message has finished arriving.
    ///
    /// ``UDSS_LLR_0036`` indicates it to the application, successful or not;
    /// ``UDSS_LLR_0058`` states the kind required on a failed reception addressed to a
    /// server. A request received successfully becomes the service in progress
    /// (``UDSS_LLR_0107``, ``UDSS_LLR_0108``) and starts `tP2_Server` with
    /// `tP2_Server_Max` (``UDSS_LLR_0113``). From the controlling client it stops
    /// `tS3_Server` (``UDSS_LLR_0087``); from any other client, or in the default
    /// session, it leaves the timer alone (``UDSS_LLR_0097``, ``UDSS_LLR_0099``). A
    /// keep-alive received successfully reloads a running `tS3_Server` where it comes
    /// from the controlling client (``UDSS_LLR_0095``) and otherwise changes nothing
    /// (``UDSS_LLR_0096``). A failed reception of a request from the controlling client
    /// restarts a stopped `tS3_Server` where no service is in progress
    /// (``UDSS_LLR_0092``); a failed keep-alive changes nothing (``UDSS_LLR_0096``).
    /// ``UDSS_LLR_0030`` bars a server from receiving a response, which [`ServerRx`]
    /// cannot express, and from an indication naming a channel, which this signature has
    /// no parameter for.
    pub fn t_data_ind<'d>(
        &mut self,
        now: Timestamp,
        ai: Ai,
        data: &'d [u8],
        result: SResult,
        class: ServerRx,
    ) -> ServerReaction<'_, 'd, A> {
        self.expire(now);
        let from = ai.source();
        match (result, class) {
            (SResult::Ok, ServerRx::Request { .. }) => {
                // UDSS_LLR_0107 / 0108: this request is now the service in progress, and
                // UDSS_LLR_0113: its tP2_Server starts with tP2_Server_Max.
                let mut p2 = Timer::STOPPED;
                p2.start(now, self.params.p2_server_max);
                self.service = Some(InProgress {
                    peer: from,
                    anchor: None,
                    p2,
                    loaded: ServerReload::P2,
                    lead: self.params.response_pending_lead, // UDSS_LLR_0186
                    outstanding: None, // UDSS_LLR_0109: no predecessor's conf answers it
                });
                // UDSS_LLR_0087; UDSS_LLR_0097 and 0099 are the cases that fall through.
                if self.is_controlling(from) {
                    self.stop_s3();
                }
            }
            (SResult::Ok, ServerRx::KeepAlive) => {
                // UDSS_LLR_0095 — only a *running* timer is reloaded; UDSS_LLR_0096
                // otherwise.
                let running = self.session.s3_running() == Some(true);
                if running && self.is_controlling(from) {
                    self.restart_s3(now);
                }
            }
            (SResult::Transport(_), ServerRx::Request { .. }) => {
                // UDSS_LLR_0092 — a non-default session with tS3_Server stopped, a
                // request from the controlling client, and no service in progress.
                let stopped = self.session.s3_running() == Some(false);
                if stopped && self.is_controlling(from) && self.service.is_none() {
                    self.restart_s3(now);
                }
            }
            (SResult::Transport(_), ServerRx::KeepAlive) => {
                // UDSS_LLR_0096 — changes nothing.
            }
        }
        // UDSS_LLR_0036
        let indicate = ServerOutput::Indicate { ai, data, result };
        Reaction::new(self, [Some(indicate), None], Ok(()))
    }

    /// A transmission has completed.
    ///
    /// ``UDSS_LLR_0025`` — addressing and a result. ``UDSS_LLR_0059`` matches it to the
    /// outstanding association by addressing, and ``UDSS_LLR_0063`` rejects one matching
    /// none. ``UDSS_LLR_0039`` forwards it to the application as an `S_Data.conf`.
    /// Matching frees the association, and the confirmation acts on the session
    /// (``UDSS_LLR_0085``, ``UDSS_LLR_0088``, ``UDSS_LLR_0093``, ``UDSS_LLR_0098``) and,
    /// only where the service in progress submitted the transmission it confirms
    /// (``UDSS_LLR_0109``), on that service and its response timer (``UDSS_LLR_0110``,
    /// ``UDSS_LLR_0116``); a busy refusal's acts on neither (``UDSS_LLR_0187``). A
    /// confirmation that a later request from the controlling client overtook restarts no
    /// `tS3_Server`: that request's stop is the later event (``UDSS_LLR_0088``,
    /// ``UDSS_LLR_0093``). One selecting a non-default session that a later request from
    /// the requester overtook enters it with `tS3_Server` stopped (``UDSS_LLR_0085``).
    pub fn t_data_conf(
        &mut self,
        now: Timestamp,
        ai: Ai,
        result: SResult,
    ) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        // UDSS_LLR_0059 / 0063 — match by addressing, S_Mtype included, and free the
        // slot in the same expression: there is no "found but empty" branch to defend.
        let Some(sent) = self
            .associations
            .iter_mut()
            .find_map(|a| a.slot.take_if(|o| o.ai == ai))
        else {
            let rejection = Rejection::new(Cause::NoMatchingAssociation);
            return Reaction::new(self, [None, None], Err(rejection));
        };
        let class = sent.class;
        let to = ai.target();
        // UDSS_LLR_0109 — a replaced service's confirmation frees its slot and acts on
        // the session below, and leaves the service that replaced it alone.
        let answers = self.answers(sent);
        // UDSS_LLR_0088, 0093 — a later request from `to` is in progress: its 0087 stop
        // came after this response completed, so this confirmation restarts nothing.
        let superseded = !answers && self.serves(to);
        let restarts_s3 = self.is_controlling(to) && !superseded;
        let ok = result == SResult::Ok;
        match class {
            ServerTx::ResponsePending => {
                if answers {
                    if ok {
                        // UDSS_LLR_0110, 0116; 0090 leaves tS3 alone.
                        if let Some(s) = self.service.as_mut() {
                            s.outstanding = None;
                            s.anchor = Some(now);
                            s.p2.start(now, self.params.p2_star_server_max);
                            s.loaded = ServerReload::P2Star;
                            s.lead = self.params.response_pending_lead; // 0186
                        }
                    } else {
                        self.service = None; // UDSS_LLR_0109
                    }
                }
                if !ok && restarts_s3 {
                    // UDSS_LLR_0093 — "a failed transmission of a response-pending message
                    // restarts the timer as Table 10 states".
                    self.restart_s3(now);
                }
            }
            // UDSS_LLR_0187 — freeing the association above is all a busy refusal's
            // does; UDSS_LLR_0091 — an unsolicited one's touches no tS3 and answers no
            // service.
            ServerTx::BusyRepeatRequest
            | ServerTx::FinalResponse {
                solicitation: Solicitation::Unsolicited,
                ..
            } => {}
            ServerTx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session,
            } => {
                if answers {
                    self.service = None; // UDSS_LLR_0109
                }
                match (ok, session) {
                    (true, Some(SessionSelection::NonDefault)) => {
                        self.enter_non_default(now, to); // UDSS_LLR_0085
                        if superseded {
                            // UDSS_LLR_0085 — the later request restarts tS3 at its
                            // own completion (0088, 0089); started now, it would run
                            // through it.
                            self.stop_s3();
                        }
                    }
                    (true, Some(SessionSelection::Default)) => {
                        self.session = Session::Default; // UDSS_LLR_0098
                    }
                    (true, None) => {
                        // UDSS_LLR_0088
                        if restarts_s3 {
                            self.restart_s3(now);
                        }
                    }
                    (false, _) => {
                        // UDSS_LLR_0093; 0094 — nothing is retransmitted.
                        if restarts_s3 {
                            self.restart_s3(now);
                        }
                    }
                }
            }
        }
        // UDSS_LLR_0037, 0039
        Reaction::new(
            self,
            [Some(ServerOutput::Confirm { ai, result }), None],
            Ok(()),
        )
    }

    /// Report that a received request is handled and no response will be transmitted.
    ///
    /// ``UDSS_LLR_0074`` — an act of the caller, not a primitive. ISO 14229-2:2021 9.5
    /// Table 6 restarts `tS3_Server` on completion where no response is required, and no
    /// message is transmitted in that case, so without this input the session layer
    /// cannot detect it and a suppressed-response request in a non-default session would
    /// never restart the timer.
    ///
    /// `ai` is the request's addressing. A report for a request not marked keep-alive
    /// that answers the service in progress ends it (``UDSS_LLR_0109``) and with it stops
    /// `tP2_Server` (``UDSS_LLR_0115``). A request selecting a non-default session enters
    /// it with the requester as controlling client and starts `tS3_Server`
    /// (``UDSS_LLR_0086``); one selecting the default session enters the default session
    /// (``UDSS_LLR_0098``); any other, from the controlling client, restarts `tS3_Server`
    /// (``UDSS_LLR_0089``). A keep-alive report changes nothing (``UDSS_LLR_0096``). The
    /// reaction carries no output of its own.
    pub fn completion_report(
        &mut self,
        now: Timestamp,
        ai: Ai,
        class: ServerRx,
    ) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        let from = ai.source();
        if let ServerRx::Request { session } = class {
            if self.serves(from) {
                self.service = None; // UDSS_LLR_0109, and 0115: its tP2 goes with it.
            }
            match session {
                Some(SessionSelection::NonDefault) => {
                    self.enter_non_default(now, from); // UDSS_LLR_0086
                }
                Some(SessionSelection::Default) => {
                    self.session = Session::Default; // UDSS_LLR_0098
                }
                None => {
                    // UDSS_LLR_0089
                    if self.is_controlling(from) {
                        self.restart_s3(now);
                    }
                }
            }
        }
        // UDSS_LLR_0096 for KeepAlive; UDSS_LLR_0074 — no output in any case.
        Reaction::new(self, [None, None], Ok(()))
    }

    /// Supply a timestamp on its own.
    ///
    /// ``UDSS_LLR_0010`` — a timestamp accompanies every other input "and also supplied
    /// on its own". This is that input; the standard names no primitive for it.
    /// ``UDSS_LLR_0079`` makes expiry evaluated only when a timestamp is supplied, so
    /// this is how a timer that has run out is noticed when nothing else is happening —
    /// `tP2_Server` the response-pending lead before it runs out (``UDSS_LLR_0186``).
    pub fn tick(&mut self, now: Timestamp) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        Reaction::new(self, [None, None], Ok(()))
    }

    /// The earliest timestamp at which a supplied timestamp could expire a timer.
    ///
    /// ``UDSS_LLR_0080`` — `None` where no timer is running. This is a query the caller
    /// reads for itself, not an output in ``UDSS_LLR_0011``'s sense, which is why it
    /// takes `&self` and produces no reaction. Without it a caller can only poll, which
    /// rounds every timing decision to its tick period. For `tP2_Server` it is the
    /// instant ``UDSS_LLR_0117`` indicates the overrun, the response-pending lead of
    /// ``UDSS_LLR_0186`` before the window closes.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let s3 = match self.session {
            Session::NonDefault { s3, .. } => s3.deadline(),
            Session::Default => None,
        };
        let p2 = self.service.and_then(|s| s.p2.deadline_by(s.lead));
        match (s3, p2) {
            (Some(a), Some(b)) => Some(earlier(a, b)),
            (a, b) => a.or(b),
        }
    }
}

impl<const A: usize> crate::sealed::Sealed for Server<A> {}

impl<'d, const A: usize> crate::reaction::Drain<'d, ServerOutput<'d>> for Server<A> {
    fn next_expiry(&mut self) -> Option<ServerOutput<'d>> {
        if let Some(client) = self.s3_expiry.take() {
            return Some(ServerOutput::SessionTimeout { client });
        }
        self.p2_expiry
            .take()
            .map(|(peer, loaded)| ServerOutput::ResponseOverrun {
                sa: peer.address,
                ae: peer.extension,
                loaded,
            })
    }
}
