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

/// One slot of the association storage ``UDSS_LLR_0059`` requires.
///
/// The association binds an `S_Data.req`'s classification and addressing to the
/// `T_Data.req` produced from it and the `T_Data.conf` that reports its outcome, from the
/// request until the confirmation. ``UDSS_LLR_0060`` permits at most one outstanding per
/// addressing; ``UDSS_LLR_0062`` rejects a request for which none is free, so the array's
/// length is the server's capacity.
/// Storage is moved into the instance, never duplicated: a copy of an outstanding
/// association is an association the session layer does not know it has. `Copy` and
/// equality are deliberately absent for that reason, and the array is built from
/// [`Association::EMPTY`] rather than from a copy.
#[derive(Debug)]
pub struct Association {
    _reserved: (),
}

impl Association {
    /// A free slot.
    ///
    /// ``UDSS_LLR_0064`` — no association is outstanding on initialisation.
    pub const EMPTY: Self = Self { _reserved: () };
}

/// What a server produces for the caller to retrieve.
///
/// ``UDSS_LLR_0012`` enumerates the standard's own outputs and leaves the enumeration
/// open, so that outputs the standard does not define reach the application by the same
/// mechanism. That requirement is what `#[non_exhaustive]` here rests on: it is one of
/// only two in the set — ``UDSS_LLR_0010`` for inputs is the other — that states an open
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
pub type ServerReaction<'s, 'd, T = ()> = Reaction<'s, 'd, ServerOutput<'d>, T>;

/// The session layer in the server role.
///
/// ``UDSS_LLR_0008`` — all state lives here, in the caller-supplied storage this owns;
/// nothing is retained anywhere else between inputs.
#[derive(Debug)]
pub struct Server<const A: usize> {
    _associations: [Association; A],
    _params: ServerParams,
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
            _associations: associations,
            _params: params,
        }
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
    ) -> ServerReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0040, 0043: {now:?} {parameter:?}")
        }
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
    ) -> ServerReaction<'_, 'd> {
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
    ) -> ServerReaction<'_, 'static> {
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
    ) -> ServerReaction<'_, 'd> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!(
                "UDSS_LLR_0036: {now:?} {ai:?} {} {result:?} {class:?}",
                data.len()
            )
        }
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
    ) -> ServerReaction<'_, 'static> {
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
    ) -> ServerReaction<'_, 'static> {
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
    pub fn tick(&mut self, now: Timestamp) -> ServerReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0010, 0079: {now:?}")
        }
    }

    /// The earliest timestamp at which a supplied timestamp could expire a timer.
    ///
    /// ``UDSS_LLR_0080`` — `None` where no timer is running. This is a query the caller
    /// reads for itself, not an output in ``UDSS_LLR_0011``'s sense, which is why it
    /// takes `&self` and produces no reaction. Without it a caller can only poll, which
    /// rounds every timing decision to its tick period.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Timestamp> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0080")
        }
    }
}
