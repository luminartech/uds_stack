//! The server role.
//!
//! ``UDSS_LLR_0029`` fixes an instance's role at creation and observes that "a node that
//! is both, a gateway or a tester under test, is two instances". This crate makes that
//! literal: [`Server`] and [`crate::Client`] are separate types, so the cross-role inputs
//! ``UDSS_LLR_0030`` lists are unrepresentable rather than rejected — the discharge
//! ``UDSS_LLR_0027`` describes when it says an interface in which an identifier cannot be
//! omitted satisfies the requirement without a check.

use crate::addressing::{Address, AddressExtension, Ai, PeerIdentity};
use crate::classification::{ServerRx, ServerTx};
use crate::params::{ServerParameter, ServerParams, ServerReload};
use crate::reaction::Reaction;
use crate::result::SResult;
use crate::time::Timestamp;
use crate::timer::{Expiry, Timer};

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
    #[expect(dead_code, reason = "read by the association bodies of Task 5")]
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
    NonDefault { client: PeerIdentity, s3: Timer },
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
/// ``UDSS_LLR_0101``'s `tP2_Server` with the parameter it carries. The timer lives here
/// because ``UDSS_LLR_0113``–``0117`` only ever run it for a service in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InProgress {
    peer: PeerIdentity,
    anchor: Option<Timestamp>,
    p2: Timer,
    loaded: ServerReload,
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
    /// `tP2_Server` expired with no response transmitted.
    ///
    /// ``UDSS_LLR_0117`` — an overrun the session layer can observe and cannot correct,
    /// so it reports and the application acts. ISO 14229-2 states no session layer action.
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
    #[expect(dead_code, reason = "read by the association bodies of Task 5")]
    associations: [Association; A],
    params: ServerParams,
    session: Session,
    service: Option<InProgress>,
    /// Expiry snapshots: taken at the instant of expiry, because the expiry itself
    /// discards the facts the indication names (spec §2.1).
    s3_expiry: Option<PeerIdentity>,
    p2_expiry: Option<(PeerIdentity, ServerReload)>,
}

impl<const A: usize> Server<A> {
    /// Create a server.
    ///
    /// ``UDSS_LLR_0032`` — creation supplies the association storage of
    /// ``UDSS_LLR_0059`` and the `tS3_Server`, `tP2_Server_Max` and `tP2*_Server_Max`
    /// parameters of ``UDSS_LLR_0042``, which have no defaults. The storage is supplied
    /// by value: ``UDSS_LLR_0004`` forbids allocation and the number of peers is a
    /// property of the deployment, so the caller sizes it as `A` and hands it over.
    /// ``UDSS_LLR_0008`` is satisfied in both its branches at once — the caller supplies
    /// the storage, and it then lives in the instance. `A` is the capacity
    /// ``UDSS_LLR_0062`` rejects against.
    #[must_use]
    pub const fn new(associations: [Association; A], params: ServerParams) -> Self {
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
    /// Sweeps the previous input's unreported snapshots first (spec §2.1 item 5).
    fn expire(&mut self, now: Timestamp) {
        self.s3_expiry = None;
        self.p2_expiry = None;
        if let Session::NonDefault { client, s3 } = self.session
            && s3.expired(now, Expiry::Reaches)
        {
            // UDSS_LLR_0100
            self.s3_expiry = Some(client);
            self.session = Session::Default;
        }
        if let Some(service) = self.service.as_mut()
            && service.p2.expired(now, Expiry::Reaches)
        {
            // UDSS_LLR_0117 — stop, and report the service and the parameter.
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
    #[expect(dead_code, reason = "used by the bodies of Tasks 4-6")]
    fn enter_non_default(&mut self, now: Timestamp, client: PeerIdentity) {
        let mut s3 = Timer::STOPPED;
        s3.start(now, self.params.s3_server);
        self.session = Session::NonDefault { client, s3 };
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
        }
        Reaction::new(self, [None, None], Ok(()))
    }

    /// Request transmission of a response.
    ///
    /// ``UDSS_LLR_0033``. ``UDSS_LLR_0054`` rejects a length differing from the data
    /// supplied, which passing a slice discharges. ``UDSS_LLR_0118`` and
    /// ``UDSS_LLR_0119`` reject a response-pending message that is unconfirmed or too
    /// soon.
    pub fn s_data_req<'d>(
        &mut self,
        now: Timestamp,
        ai: Ai,
        data: &'d [u8],
        class: ServerTx,
    ) -> ServerReaction<'_, 'd, A> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0033: {now:?} {ai:?} {} {class:?}", data.len())
        }
    }

    /// A message has started arriving.
    ///
    /// ``UDSS_LLR_0023`` — addressing and no data, length or result.
    /// ``UDSS_LLR_0038`` keeps it inside the session layer; it is never forwarded.
    /// ``UDSS_LLR_0030`` bars a server from receiving a response, which [`ServerRx`]
    /// cannot express, and from an indication naming a channel, which this signature has
    /// no parameter for.
    pub fn t_data_som_ind(
        &mut self,
        now: Timestamp,
        ai: Ai,
        class: ServerRx,
    ) -> ServerReaction<'_, 'static, A> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0023, 0045: {now:?} {ai:?} {class:?}")
        }
    }

    /// A message has finished arriving.
    ///
    /// ``UDSS_LLR_0036`` indicates it to the application; ``UDSS_LLR_0058`` states the
    /// kind required on a failed reception addressed to a server. ``UDSS_LLR_0030`` bars a
    /// server from receiving a response, which [`ServerRx`] cannot express, and from an
    /// indication naming a channel, which this signature has no parameter for.
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
    pub fn t_data_conf(
        &mut self,
        now: Timestamp,
        ai: Ai,
        result: SResult,
    ) -> ServerReaction<'_, 'static, A> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0025, 0059: {now:?} {ai:?} {result:?}")
        }
    }

    /// Report that a received request is handled and no response will be transmitted.
    ///
    /// ``UDSS_LLR_0074`` — an act of the caller, not a primitive. ISO 14229-2:2021 9.5
    /// Table 6 restarts `tS3_Server` on completion where no response is required, and no
    /// message is transmitted in that case, so without this input the session layer
    /// cannot detect it and a suppressed-response request in a non-default session would
    /// never restart the timer.
    pub fn completion_report(
        &mut self,
        now: Timestamp,
        ai: Ai,
        class: ServerRx,
    ) -> ServerReaction<'_, 'static, A> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0074: {now:?} {ai:?} {class:?}")
        }
    }

    /// Supply a timestamp on its own.
    ///
    /// ``UDSS_LLR_0010`` — a timestamp accompanies every other input "and also supplied
    /// on its own". This is that input; the standard names no primitive for it.
    /// ``UDSS_LLR_0079`` makes expiry evaluated only when a timestamp is supplied, so
    /// this is how a timer that has run out is noticed when nothing else is happening.
    pub fn tick(&mut self, now: Timestamp) -> ServerReaction<'_, 'static, A> {
        self.expire(now);
        Reaction::new(self, [None, None], Ok(()))
    }

    /// The earliest timestamp at which a supplied timestamp could expire a timer.
    ///
    /// ``UDSS_LLR_0080`` — `None` where no timer is running. This is a query the caller
    /// reads for itself, not an output in ``UDSS_LLR_0011``'s sense, which is why it
    /// takes `&self` and produces no reaction. Without it a caller can only poll, which
    /// rounds every timing decision to its tick period.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let s3 = match self.session {
            Session::NonDefault { s3, .. } => s3.deadline(Expiry::Reaches),
            Session::Default => None,
        };
        let p2 = self.service.and_then(|s| s.p2.deadline(Expiry::Reaches));
        match (s3, p2) {
            (Some(a), Some(b)) => Some(earlier(a, b)),
            (a, b) => a.or(b),
        }
    }
}

/// The earlier of two wrapping timestamps (``UDSS_LLR_0019``): `a` is earlier when the
/// modular difference `a - b` lands in the upper half of the range.
fn earlier(a: Timestamp, b: Timestamp) -> Timestamp {
    if a.0.wrapping_sub(b.0) > u32::MAX / 2 {
        a
    } else {
        b
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

#[cfg(test)]
mod tests {
    use super::earlier;
    use crate::time::Timestamp;

    /// ``UDSS_LLR_0019`` — the earlier deadline is chosen by modular distance, so one
    /// just past the wrap is later than one just before it.
    #[test]
    fn the_earlier_deadline_is_chosen_across_the_wrap() {
        assert_eq!(earlier(Timestamp(10), Timestamp(20)), Timestamp(10));
        assert_eq!(earlier(Timestamp(20), Timestamp(10)), Timestamp(10));
        let before = Timestamp(u32::MAX - 5);
        let after = Timestamp(5);
        assert_eq!(earlier(before, after), before);
        assert_eq!(earlier(after, before), before);
    }
}
