//! The client role.
//!
//! ``UDSS_LLR_0029`` fixes the role at creation; see [`crate::Server`] for why the two
//! roles are separate types. ``UDSS_LLR_0031`` lists what a client rejects, and each item
//! is unrepresentable here: there is no `completion_report`, and [`ClientTx`] and
//! [`ClientRx`] cannot express the server's kinds.
//!
//! # Storage
//!
//! ``UDSS_LLR_0004`` forbids allocation, so storage is supplied by the caller, by value.
//! `Client` carries no lifetime as a result: nothing here is borrowed, so nothing here can
//! outlive it or be reasoned about across it. The physical and functional arrays are
//! supplied whole at [`Client::new`], before any channel exists; ``UDSS_LLR_0121`` makes
//! opening a channel, not supplying that storage, the act that brings the channel into
//! being.
//!
//! The physical and functional arrays are separate because the two channel kinds hold
//! different state. ``UDSS_LLR_0139`` gives a functional channel a responder table and
//! gives a physical channel none; ``UDSS_LLR_0151`` gives a physical channel a
//! `tS3_Client` timer and gives a functional channel none. Splitting [`PhysicalSlot`] from
//! [`FunctionalSlot`] keeps both facts true by construction rather than by a runtime
//! check, and it means a physical channel is never charged storage for a table it must
//! not keep. That is earned rather than assumed: [`Client::open_physical_channel`] and
//! [`Client::open_functional_channel`] each take a [`crate::ChannelAddressing`], which
//! carries no `ta_type`, and each supplies ``UDSS_LLR_0049``'s matching value itself, so a
//! channel stored in one array can never carry the other kind's `S_AI[TAtype]`.
//!
//! `R` sizes every functional channel's responder table alike. A client with no
//! functional channels at all sets `FUNC` to `0`, pays no storage for one, and leaves `R`
//! to its default of `0` rather than naming a table size that would mean nothing:
//! `Client<K, 2, 0>`.
//!
//! ```
//! use uds_session::{Client, FunctionalKeepAlive, FunctionalSlot, PhysicalSlot};
//!
//! let client: Client<FunctionalKeepAlive, 4, 1, 8> = Client::new(
//!     [PhysicalSlot::EMPTY; 4],
//!     [FunctionalSlot::EMPTY; 1],
//!     FunctionalKeepAlive::new(2_000),
//! );
//! # let _ = client;
//! ```

use crate::addressing::{
    Address, AddressExtension, Ai, ChannelAddressing, PeerIdentity, TaType,
};
use crate::classification::{ClientRx, ClientTx, ExpectedResponses, Solicitation};
use crate::keep_alive::{
    Event, FunctionalKeepAlive, KeepAliveMode, PhysicalKeepAlive, PhysicalSession, Site,
};
use crate::params::{ChannelParameter, ChannelParams, ChannelReload};
use crate::reaction::Reaction;
use crate::rejection::{Cause, Content, Rejection};
use crate::result::SResult;
use crate::time::{Timestamp, earlier};
use crate::timer::{Exceeds, Reaches, Timer};

/// One entry of a functional channel's responder table.
///
/// ``UDSS_LLR_0139`` — keyed by a responder's peer identity, recording whether a
/// start-of-message is open and whether a response-pending message is outstanding.
/// ISO 14229-2:2021 9.6 Table 7 allots the client one timer per channel and no storage
/// for either fact, which is why this requirement is derived.
/// Storage is moved into the instance, never duplicated — see [`crate::Association`].
#[derive(Debug)]
pub struct ResponderSlot {
    entry: Option<Responder>,
}

impl ResponderSlot {
    /// A free entry.
    pub const EMPTY: Self = Self { entry: None };
}

#[derive(Debug, Clone, Copy)]
struct Responder {
    peer: PeerIdentity,
    som_open: bool,
    pending: bool,
}

/// One physical channel's storage.
///
/// ``UDSS_LLR_0126`` — every fact a document of this set keeps per channel lives here:
/// the channel's `tP_Client` and spacing timers and their parameters, whether a request
/// is in progress and its addressing and classification, the response count, the one
/// association ``UDSS_LLR_0059`` holds and whether it is abandoned, the repeat count of
/// ``UDSS_LLR_0173``, whether a start-of-message is open, and in physical keep-alive the
/// `tS3_Client` timer and session fact of ``UDSS_LLR_0151``.
///
/// It holds no responder table: ``UDSS_LLR_0139`` gives one to functional channels alone.
/// `K` is the client's keep-alive mode: in [`FunctionalKeepAlive`] the slot holds no
/// keep-alive state, and in [`PhysicalKeepAlive`] it holds the channel's own `tS3_Client`
/// timer, reload and session fact. [`PhysicalSlot::EMPTY`] infers `K` from the client it
/// is supplied to, so a caller names it only where they annotate the type.
#[derive(Debug)]
pub struct PhysicalSlot<K: KeepAliveMode> {
    channel: Option<Physical<K>>,
    timed_out: Option<TimedOut>,
    /// ``UDSS_LLR_0162`` — the channel's keep-alive fell due at this input's timestamp.
    keep_alive_due: Option<PhysicalChannelId>,
}

impl<K: KeepAliveMode> PhysicalSlot<K> {
    /// A slot holding no channel.
    pub const EMPTY: Self = Self {
        channel: None,
        timed_out: None,
        keep_alive_due: None,
    };
}

/// One functional channel's storage, with room for `R` responders.
///
/// ``UDSS_LLR_0126`` holds the per-channel facts, as [`PhysicalSlot`] lists them, except
/// the `tS3_Client` timer and session fact, which ``UDSS_LLR_0151`` gives to physical
/// channels alone, and the channel-level start-of-message fact, which ``UDSS_LLR_0139``
/// holds per responder here instead. ``UDSS_LLR_0139`` adds the responder table, whose
/// capacity is the number of entries this storage holds — `R`.
#[derive(Debug)]
pub struct FunctionalSlot<const R: usize> {
    channel: Option<Channel>,
    responders: [ResponderSlot; R],
    timed_out: Option<TimedOut>,
}

impl<const R: usize> FunctionalSlot<R> {
    /// A slot holding no channel, its responder table empty.
    ///
    /// ``UDSS_LLR_0142`` — a channel's responder table holds no entry when it is opened.
    pub const EMPTY: Self = Self {
        channel: None,
        responders: [ResponderSlot::EMPTY; R],
        timed_out: None,
    };
}

/// Identifies a physical channel of this client.
///
/// ``UDSS_LLR_0121`` — returned when the caller opens one, valid until it is withdrawn,
/// and never reissued: ids come from one client-wide counter, so a withdrawn channel's
/// handle identifies no later one. The value is private and has no accessor, which is a
/// promise: nothing about it is meaningful to a caller. `PartialOrd`/`Ord` are not
/// derived, since nothing needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalChannelId(u32);

/// Identifies a functional channel of this client.
///
/// ``UDSS_LLR_0121`` — returned when the caller opens one, valid until it is withdrawn,
/// and never reissued: ids come from one client-wide counter, so a withdrawn channel's
/// handle identifies no later one. The value is private and has no accessor, which is a
/// promise: nothing about it is meaningful to a caller. `PartialOrd`/`Ord` are not
/// derived, since nothing needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionalChannelId(u32);

/// Identifies a channel of either kind.
///
/// ``UDSS_LLR_0049`` distinguishes the two kinds and this set gives them different
/// state — ``UDSS_LLR_0139`` a responder table to a functional channel and
/// ``UDSS_LLR_0151`` a `tS3_Client` to a physical one — so the identity carries the kind.
/// Operations that act on either kind take anything that converts into this, so either
/// kind's own identity passes directly; those that act on one kind take that kind's own
/// identity, which is why no setting can name a channel of the wrong kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelId {
    /// A physical channel.
    Physical(PhysicalChannelId),
    /// A functional channel.
    Functional(FunctionalChannelId),
}

impl From<PhysicalChannelId> for ChannelId {
    /// Widen a physical channel's identity to either kind's.
    ///
    /// ``UDSS_LLR_0121`` — the same channel. A kind-agnostic method such as
    /// [`Client::withdraw_channel`] takes `impl Into<ChannelId>`, so this conversion
    /// happens at the call rather than in the caller's own code.
    fn from(id: PhysicalChannelId) -> Self {
        Self::Physical(id)
    }
}

impl From<FunctionalChannelId> for ChannelId {
    /// Widen a functional channel's identity to either kind's.
    ///
    /// ``UDSS_LLR_0121`` — the same channel. A kind-agnostic method such as
    /// [`Client::withdraw_channel`] takes `impl Into<ChannelId>`, so this conversion
    /// happens at the call rather than in the caller's own code.
    fn from(id: FunctionalChannelId) -> Self {
        Self::Functional(id)
    }
}

/// One open channel's state, of either kind (``UDSS_LLR_0126``).
#[derive(Debug, Clone, Copy)]
struct Channel {
    id: u32,
    ai: Ai,
    params: ChannelParams,
    /// `tP_Client` — ``UDSS_LLR_0120``, ``UDSS_LLR_0148``.
    response: Timer<Exceeds>,
    loaded: ChannelReload,
    /// ``UDSS_LLR_0059``, ``UDSS_LLR_0060`` — the one association a channel holds.
    sent: Option<Sent>,
    /// ``UDSS_LLR_0128`` — apart from `response`, which a response-pending
    /// start-of-message stops without ending the request.
    request: Option<InProgress>,
    /// ``UDSS_LLR_0164`` — `tP3_Client_Phys` or `tP3_Client_Func`.
    spacing: Timer<Reaches>,
    /// ``UDSS_LLR_0173``.
    repeats: u8,
}

#[derive(Debug, Clone, Copy)]
struct InProgress {
    class: ClientTx,
    /// ``UDSS_LLR_0138`` — solicited final responses since the confirmation.
    received: u16,
}

#[derive(Debug, Clone, Copy)]
struct Sent {
    class: ClientTx,
    /// ``UDSS_LLR_0180`` — its channel was reset while it was outstanding.
    abandoned: bool,
}

/// An expired `tP_Client`, kept in the slot rather than the channel so that a withdrawal
/// at the same timestamp still indicates it (``UDSS_LLR_0081``).
#[derive(Debug, Clone, Copy)]
struct TimedOut {
    ai: Ai,
    loaded: ChannelReload,
}

impl Channel {
    /// ``UDSS_LLR_0127``, ``UDSS_LLR_0167`` — no request, no association, no timer.
    const fn opened(id: u32, ai: Ai, params: ChannelParams) -> Self {
        Self {
            id,
            ai,
            params,
            response: Timer::STOPPED,
            loaded: ChannelReload::Default,
            sent: None,
            request: None,
            spacing: Timer::STOPPED,
            repeats: 0,
        }
    }

    /// The handle naming this channel; its kind is its `TAtype` (``UDSS_LLR_0049``).
    const fn handle(&self) -> ChannelId {
        match self.ai.ta_type {
            TaType::Physical => ChannelId::Physical(PhysicalChannelId(self.id)),
            TaType::Functional => ChannelId::Functional(FunctionalChannelId(self.id)),
        }
    }

    /// ``UDSS_LLR_0148`` — stop an exceeded `tP_Client`, ending the request
    /// (``UDSS_LLR_0128``), and say what it timed.
    fn expire(&mut self, now: Timestamp) -> Option<TimedOut> {
        if self.spacing.expired(now) {
            self.spacing.stop(); // UDSS_LLR_0166, and nothing else
        }
        if !self.response.expired(now) {
            return None;
        }
        self.close();
        Some(TimedOut {
            ai: self.ai,
            loaded: self.loaded,
        })
    }

    /// ``UDSS_LLR_0043`` — a running timer keeps the value it was loaded with
    /// (``UDSS_LLR_0076``).
    const fn set(&mut self, parameter: ChannelParameter) {
        match parameter {
            ChannelParameter::Reloads(reloads) => self.params.reloads = reloads,
            ChannelParameter::DefaultReload(v) => self.params.reloads.default_reload = v,
            ChannelParameter::EnhancedReload(v) => self.params.reloads.enhanced_reload = v,
            ChannelParameter::Spacing(v) => self.params.spacing = v,
        }
    }

    fn deadlines(&self) -> [Option<Timestamp>; 2] {
        [self.response.deadline(), self.spacing.deadline()]
    }

    /// ``UDSS_LLR_0176`` — the count an accepted request leaves.
    const fn count(&mut self, class: ClientTx) {
        match class {
            ClientTx::Request { repeat: false, .. } => self.repeats = 0,
            ClientTx::Request { repeat: true, .. } => {
                self.repeats = self.repeats.saturating_add(1);
            }
            ClientTx::KeepAlive { .. } => {}
        }
    }

    /// ``UDSS_LLR_0180`` for the state every channel holds; the spacing timer runs on.
    fn reset(&mut self) {
        self.close();
        if let Some(sent) = self.sent.as_mut() {
            sent.abandoned = true;
        }
        self.repeats = 0;
    }

    /// ``UDSS_LLR_0128`` — the request ends and its window with it.
    fn close(&mut self) {
        self.response.stop();
        self.request = None;
    }

    fn restart(&mut self, now: Timestamp, which: ChannelReload) {
        self.response
            .start(now, self.params.reloads.value_for(which));
        self.loaded = which;
    }

    /// ``UDSS_LLR_0138`` — count one solicited final response; whether it is the last
    /// the request expects.
    fn completes_count(&mut self) -> bool {
        self.request.as_mut().is_some_and(|r| {
            r.received = r.received.saturating_add(1);
            r.class
                .expected()
                .exact()
                .is_some_and(|n| n.get() == r.received)
        })
    }
}

const fn solicited_final(class: ClientRx) -> bool {
    matches!(
        class,
        ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            ..
        }
    )
}

#[derive(Debug)]
struct Physical<K: KeepAliveMode> {
    core: Channel,
    /// ``UDSS_LLR_0045``, ``UDSS_LLR_0126`` — the channel's peer, not recorded.
    som_open: bool,
    keep_alive: K::Channel,
}

impl<K: KeepAliveMode> Physical<K> {
    /// ``UDSS_LLR_0136`` at a start-of-message.
    fn on_som(&mut self, class: ClientRx) {
        self.som_open = true; // UDSS_LLR_0045
        if self.core.request.is_none() {
            return;
        }
        if solicited_final(class) {
            self.core.close();
        } else if class == ClientRx::ResponsePending {
            self.core.response.stop();
        }
    }

    /// ``UDSS_LLR_0136`` and ``UDSS_LLR_0144`` at a completed message.
    fn on_ind(&mut self, now: Timestamp, result: SResult, class: Option<ClientRx>) {
        let first = !self.som_open;
        self.som_open = false; // UDSS_LLR_0045, UDSS_LLR_0130
        if self.core.request.is_none() {
            return;
        }
        match (result, class) {
            (SResult::Transport(_), _) => self.core.close(),
            (SResult::Ok, Some(c)) if first && solicited_final(c) => self.core.close(),
            (SResult::Ok, Some(ClientRx::ResponsePending)) => {
                self.core.restart(now, ChannelReload::Enhanced);
            }
            (SResult::Ok, _) => {}
        }
    }
}

impl<const R: usize> FunctionalSlot<R> {
    fn entry(&mut self, peer: PeerIdentity) -> Option<&mut Responder> {
        self.responders
            .iter_mut()
            .filter_map(|r| r.entry.as_mut())
            .find(|e| e.peer == peer)
    }

    /// ``UDSS_LLR_0140`` — the responder's entry, created where it has none and one is
    /// free; `None` is ``UDSS_LLR_0143``'s full table.
    fn entry_or_insert(&mut self, peer: PeerIdentity) -> Option<&mut Responder> {
        let at = self
            .responders
            .iter()
            .position(|r| r.entry.is_some_and(|e| e.peer == peer))
            .or_else(|| self.responders.iter().position(|r| r.entry.is_none()))?;
        self.responders.get_mut(at).map(|r| {
            r.entry.get_or_insert(Responder {
                peer,
                som_open: false,
                pending: false,
            })
        })
    }

    /// ``UDSS_LLR_0140`` — an entry holding neither fact is released.
    fn release_idle(&mut self) {
        for r in &mut self.responders {
            if r.entry.is_some_and(|e| !e.som_open && !e.pending) {
                r.entry = None;
            }
        }
    }

    /// ``UDSS_LLR_0141`` — at the end of the request every pending fact clears.
    fn end_request(&mut self) {
        for e in self.responders.iter_mut().filter_map(|r| r.entry.as_mut()) {
            e.pending = false;
        }
        self.release_idle();
    }

    /// ``UDSS_LLR_0178`` — some responder's start-of-message is open.
    fn still_arriving(&self) -> bool {
        self.responders
            .iter()
            .any(|r| r.entry.is_some_and(|e| e.som_open))
    }

    /// ``UDSS_LLR_0145``.
    fn reload_in_force(&self) -> ChannelReload {
        if self
            .responders
            .iter()
            .any(|r| r.entry.is_some_and(|e| e.pending))
        {
            ChannelReload::Enhanced
        } else {
            ChannelReload::Default
        }
    }

    fn in_progress(&self) -> bool {
        self.channel.as_ref().is_some_and(|c| c.request.is_some())
    }

    /// ``UDSS_LLR_0137`` at a start-of-message; whether the table had no room
    /// (``UDSS_LLR_0143``).
    fn on_som(&mut self, now: Timestamp, peer: PeerIdentity, class: ClientRx) -> bool {
        let full = match self.entry_or_insert(peer) {
            Some(e) => {
                e.pending = false; // UDSS_LLR_0146
                e.som_open = true; // UDSS_LLR_0045
                false
            }
            None => true,
        };
        let restarts = solicited_final(class) || class == ClientRx::ResponsePending;
        let in_force = self.reload_in_force(); // UDSS_LLR_0147: after the clear
        if let Some(channel) = self.channel.as_mut()
            && channel.request.is_some()
            && restarts
        {
            channel.restart(now, in_force);
        }
        full
    }

    /// ``UDSS_LLR_0137``, ``UDSS_LLR_0138``, ``UDSS_LLR_0144`` and ``UDSS_LLR_0146`` at
    /// a completed message; whether the table had no room (``UDSS_LLR_0143``).
    fn on_ind(
        &mut self,
        now: Timestamp,
        peer: PeerIdentity,
        result: SResult,
        class: Option<ClientRx>,
    ) -> bool {
        let first = self.entry(peer).is_none_or(|e| {
            let first = !e.som_open;
            e.som_open = false; // UDSS_LLR_0045
            if first {
                e.pending = false; // UDSS_LLR_0146
            }
            first
        });
        let pending = result == SResult::Ok && class == Some(ClientRx::ResponsePending);
        let full = pending
            && self.in_progress()
            && match self.entry_or_insert(peer) {
                Some(e) => {
                    e.pending = true; // UDSS_LLR_0146, UDSS_LLR_0147
                    false
                }
                None => true,
            };
        let in_force = self.reload_in_force();
        let mut ends = false;
        if let Some(channel) = self.channel.as_mut()
            && channel.request.is_some()
        {
            match (result, class) {
                (SResult::Transport(_), _) => ends = true, // UDSS_LLR_0137
                (SResult::Ok, Some(c)) if solicited_final(c) => {
                    if channel.completes_count() {
                        ends = true; // UDSS_LLR_0138
                    } else if first {
                        channel.restart(now, in_force); // UDSS_LLR_0137
                    }
                }
                (SResult::Ok, Some(ClientRx::ResponsePending)) => {
                    channel.restart(now, ChannelReload::Enhanced); // UDSS_LLR_0144
                }
                (SResult::Ok, _) => {}
            }
            if ends {
                channel.close(); // UDSS_LLR_0128
            }
        }
        if ends {
            self.end_request();
        } else {
            self.release_idle();
        }
        full
    }
}

/// ``UDSS_LLR_0143`` — the responder `ai` came from had no room in `channel`'s table.
const fn capacity(channel: FunctionalChannelId, ai: Ai) -> ClientOutput<'static> {
    ClientOutput::Capacity {
        channel,
        sa: ai.sa,
        ae: ai.mtype.address_extension(),
    }
}

const NO_SUCH_CHANNEL: Rejection = Rejection::new(Cause::NoSuchChannel);

/// What a client produces for the caller to retrieve.
///
/// ``UDSS_LLR_0012`` — the standard's own outputs, plus the ones it does not define. That
/// requirement states an open enumeration, which is what `#[non_exhaustive]` rests on
/// here; see [`crate::ServerOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientOutput<'d> {
    /// `T_Data.req` — ``UDSS_LLR_0024``. The data is the caller's own, per
    /// ``UDSS_LLR_0014``.
    Transmit {
        /// Which channel it belongs to.
        ///
        /// A convenience: the request's addressing already determines the channel, since
        /// ``UDSS_LLR_0122`` makes an addressing unique per channel, so this field saves
        /// the caller a lookup rather than carrying information the addressing lacks.
        channel: ChannelId,
        /// Where it goes.
        ai: Ai,
        /// What to send.
        data: &'d [u8],
    },
    /// `S_Data.ind` — ``UDSS_LLR_0034`` and ``UDSS_LLR_0036``.
    Indicate {
        /// The channel the caller identified under ``UDSS_LLR_0026``.
        channel: ChannelId,
        /// Who it came from and who it was for.
        ai: Ai,
        /// The message; meaningful only where `result` is [`SResult::Ok`]
        /// (``UDSS_LLR_0035``).
        data: &'d [u8],
        /// The outcome of the reception.
        result: SResult,
    },
    /// `S_Data.conf` — ``UDSS_LLR_0037``.
    Confirm {
        /// The addressing identifying the request confirmed, per ``UDSS_LLR_0059``.
        ai: Ai,
        /// The outcome of the transmission.
        result: SResult,
    },
    /// A channel's `tP_Client` expired with the response window unfilled.
    ///
    /// ``UDSS_LLR_0148`` — ISO 14229-2:2021 9.1.2's error condition, which 9.7 Table 9
    /// heads "Timeout".
    ResponseTimeout {
        /// The addressing of the request whose window expired.
        ai: Ai,
        /// Which of the default and enhanced reload parameters the timer was carrying.
        loaded: ChannelReload,
    },
    /// A keep-alive `TesterPresent` is due.
    ///
    /// ``UDSS_LLR_0156`` in functional keep-alive, carrying no addressing because the
    /// keep-alive is client-wide; ``UDSS_LLR_0162`` in physical keep-alive, carrying the
    /// physical channel. The session layer requests it rather than composing it — it
    /// never builds a message.
    KeepAliveDue {
        /// `None` in functional keep-alive; the physical channel in physical keep-alive.
        channel: Option<PhysicalChannelId>,
    },
    /// A responder was seen that the table has no room for.
    ///
    /// ``UDSS_LLR_0143`` — the client records nothing for it and says so, rather than
    /// silently mistracking it. Where the same `T_Data.ind` also produces an
    /// [`ClientOutput::Indicate`], this precedes it. The table belongs to a functional
    /// channel alone: ``UDSS_LLR_0139`` gives a physical one no table to overflow.
    Capacity {
        /// The functional channel it arrived on.
        channel: FunctionalChannelId,
        /// The responder's `S_AI[SA]`.
        sa: Address,
        /// Its `S_AI[AE]`, where `S_Mtype` carries one.
        ae: Option<AddressExtension>,
    },
}

/// A reaction carrying client outputs.
pub type ClientReaction<
    's,
    'd,
    K,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
    T = (),
> = Reaction<'s, 'd, ClientOutput<'d>, Client<K, PHYS, FUNC, R>, T>;

/// The session layer in the client role.
///
/// ``UDSS_LLR_0008`` — all state lives here: the channel storage and keep-alive mode are
/// supplied by the caller at creation, and opening a channel supplies that channel's own
/// parameters. The arrays split by channel kind because the two kinds hold different
/// state: ``UDSS_LLR_0139`` gives a responder table to functional channels alone, and
/// ``UDSS_LLR_0151`` a `tS3_Client` to physical ones alone.
///
/// `K` is the keep-alive mode of ``UDSS_LLR_0149``, fixed at creation and held in the type
/// rather than in a field — see [`KeepAliveMode`]. It is inferred from the value passed
/// to [`Client::new`], so a caller names it only where they annotate the type.
///
/// `R` defaults to `0`, since a client with `FUNC` of `0` has no responder table to size.
#[derive(Debug)]
pub struct Client<
    K: KeepAliveMode,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize = 0,
> {
    physical: [PhysicalSlot<K>; PHYS],
    functional: [FunctionalSlot<R>; FUNC],
    keep_alive: K,
    /// ``UDSS_LLR_0156`` — the client-wide keep-alive fell due at this input's timestamp.
    keep_alive_due: bool,
    /// ``UDSS_LLR_0121``, ``UDSS_LLR_0185`` — the next handle to issue; `checked_add` on
    /// it failing is the second limb's rejection.
    next_id: u32,
}

impl<K: KeepAliveMode, const PHYS: usize, const FUNC: usize, const R: usize>
    Client<K, PHYS, FUNC, R>
{
    /// Create a client.
    ///
    /// ``UDSS_LLR_0032`` — creation supplies the keep-alive mode of ``UDSS_LLR_0149``
    /// and, in functional keep-alive, the storage of ``UDSS_LLR_0150`` and the reload of
    /// ``UDSS_LLR_0152``. Channel storage is supplied here too, by value: `PHYS` physical
    /// slots and `FUNC` functional slots of `R` responders each. A channel begins to
    /// exist when the caller opens one, under ``UDSS_LLR_0121``.
    ///
    /// `keep_alive` is [`FunctionalKeepAlive`] or [`PhysicalKeepAlive`], and fixes `K` for
    /// the life of the instance, as ``UDSS_LLR_0149`` requires.
    #[must_use]
    pub const fn new(
        physical: [PhysicalSlot<K>; PHYS],
        functional: [FunctionalSlot<R>; FUNC],
        keep_alive: K,
    ) -> Self {
        Self {
            physical,
            functional,
            keep_alive,
            keep_alive_due: false,
            next_id: 0,
        }
    }

    /// ``UDSS_LLR_0081`` — act on every expiry the timestamp causes before the input,
    /// sweeping the previous input's unreported snapshots.
    fn expire(&mut self, now: Timestamp) {
        for slot in &mut self.physical {
            slot.timed_out = None;
            slot.keep_alive_due = None;
            let Some(p) = slot.channel.as_mut() else {
                continue;
            };
            if K::expire_channel(&mut p.keep_alive, now) {
                slot.keep_alive_due = Some(PhysicalChannelId(p.core.id)); // UDSS_LLR_0162
            }
            let class = p.core.request.map(|r| r.class);
            slot.timed_out = p.core.expire(now);
            if slot.timed_out.is_some() && matches!(class, Some(ClientTx::KeepAlive { .. }))
            {
                // UDSS_LLR_0161, fifth bullet; UDSS_LLR_0079 leaves the restart to the
                // next timestamp
                let site = Site::Physical(&mut p.keep_alive);
                self.keep_alive.on(now, site, Event::KeepAliveWindowExpired);
            }
        }
        for slot in &mut self.functional {
            slot.timed_out = slot.channel.as_mut().and_then(|c| c.expire(now));
            if slot.timed_out.is_some() {
                slot.end_request(); // UDSS_LLR_0141
            }
        }
        self.keep_alive_due = self.keep_alive.expire(now);
    }

    /// Tell the keep-alive mode of `event` on `channel`.
    fn keep_alive_on(&mut self, now: Timestamp, channel: ChannelId, event: Event) {
        let Self {
            physical,
            keep_alive,
            ..
        } = self;
        let site = match channel {
            ChannelId::Physical(id) => {
                let named = physical
                    .iter_mut()
                    .filter_map(|s| s.channel.as_mut())
                    .find(|p| p.core.id == id.0);
                match named {
                    Some(p) => Site::Physical(&mut p.keep_alive),
                    None => return,
                }
            }
            ChannelId::Functional(_) => Site::Functional,
        };
        keep_alive.on(now, site, event);
    }

    fn channels(&self) -> impl Iterator<Item = &Channel> {
        let physical = self.physical.iter().filter_map(|s| s.channel.as_ref());
        let functional = self.functional.iter().filter_map(|s| s.channel.as_ref());
        physical.map(|p| &p.core).chain(functional)
    }

    fn channels_mut(&mut self) -> impl Iterator<Item = &mut Channel> {
        let physical = self.physical.iter_mut().filter_map(|s| s.channel.as_mut());
        let functional = self
            .functional
            .iter_mut()
            .filter_map(|s| s.channel.as_mut());
        physical.map(|p| &mut p.core).chain(functional)
    }

    fn channel_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels_mut().find(|c| c.handle() == id)
    }

    fn physical_mut(&mut self, id: PhysicalChannelId) -> Option<&mut Physical<K>> {
        self.physical
            .iter_mut()
            .filter_map(|s| s.channel.as_mut())
            .find(|p| p.core.id == id.0)
    }

    fn functional_mut(
        &mut self,
        id: FunctionalChannelId,
    ) -> Option<&mut FunctionalSlot<R>> {
        self.functional
            .iter_mut()
            .find(|s| s.channel.as_ref().is_some_and(|c| c.id == id.0))
    }

    /// ``UDSS_LLR_0027`` and ``UDSS_LLR_0069``, every cause stated (``UDSS_LLR_0016``).
    fn validate_ind(
        &self,
        channel: ChannelId,
        result: SResult,
        class: Option<ClientRx>,
    ) -> Result<(), Rejection> {
        let mut causes: Option<Rejection> = None;
        let mut add = |c: Cause| {
            causes = Some(causes.map_or(Rejection::new(c), |r| r.with(c)));
        };
        if !self.channels().any(|c| c.handle() == channel) {
            add(Cause::NoSuchChannel); // UDSS_LLR_0027
        }
        if result == SResult::Ok && class.is_none() {
            add(Cause::KindRequired); // UDSS_LLR_0069
        }
        causes.map_or(Ok(()), Err)
    }

    /// ``UDSS_LLR_0122`` and ``UDSS_LLR_0185``, every cause stated (``UDSS_LLR_0016``).
    fn validate_open(&self, ai: Ai, free: bool) -> Result<(), Rejection> {
        let mut causes: Option<Rejection> = None;
        let mut add = |c: Cause| {
            causes = Some(causes.map_or(Rejection::new(c), |r| r.with(c)));
        };
        if self.channels().any(|c| c.ai == ai) {
            add(Cause::DuplicateChannelAddressing); // UDSS_LLR_0122
        }
        if !free {
            add(Cause::NoChannelSlotFree); // UDSS_LLR_0185
        }
        if self.next_id.checked_add(1).is_none() {
            add(Cause::ChannelHandlesSpent); // UDSS_LLR_0185
        }
        causes.map_or(Ok(()), Err)
    }

    /// ``UDSS_LLR_0121`` — the next handle, never reissued. [`Client::validate_open`]
    /// has already refused an open with none left.
    const fn issue(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = id.saturating_add(1);
        id
    }

    fn open_physical(
        &mut self,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
        keep_alive: K::Channel,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R, PhysicalChannelId> {
        let ai = addressing.with_ta_type(TaType::Physical);
        self.expire(now);
        let free = self.physical.iter().any(|s| s.channel.is_none());
        let outcome = self.validate_open(ai, free).map(|()| {
            let id = self.issue();
            if let Some(slot) = self.physical.iter_mut().find(|s| s.channel.is_none()) {
                slot.channel = Some(Physical {
                    core: Channel::opened(id, ai, params),
                    som_open: false, // UDSS_LLR_0127
                    keep_alive,
                });
            }
            PhysicalChannelId(id)
        });
        Reaction::new(self, [None, None], outcome)
    }

    /// ``UDSS_LLR_0123`` and ``UDSS_LLR_0061``, every cause stated (``UDSS_LLR_0016``).
    fn validate_req(
        &self,
        now: Timestamp,
        ai: Ai,
        class: ClientTx,
    ) -> Result<(), Rejection> {
        let Some(channel) = self.channels().find(|c| c.ai == ai) else {
            return Err(NO_SUCH_CHANNEL); // UDSS_LLR_0123
        };
        let mut causes: Option<Rejection> = None;
        let mut add = |c: Cause| {
            causes = Some(causes.map_or(Rejection::new(c), |r| r.with(c)));
        };
        if channel.sent.is_some() {
            add(Cause::AssociationOutstanding); // UDSS_LLR_0061
        }
        if matches!(class, ClientTx::Request { repeat: true, .. }) && channel.repeats >= 2 {
            add(Cause::RepeatCountSpent); // UDSS_LLR_0177
        }
        let arriving = self
            .functional
            .iter()
            .filter(|s| s.channel.as_ref().is_some_and(|c| c.ai == ai))
            .any(FunctionalSlot::still_arriving);
        if arriving {
            add(Cause::ResponseStillArriving); // UDSS_LLR_0178
        }
        if let Some(remaining) = channel.spacing.remaining(now) {
            // UDSS_LLR_0171, with UDSS_LLR_0172's time left
            let spacing = Content::SpacingTimerRunning { remaining };
            let first = Rejection::new(Cause::SpacingTimerRunning);
            causes = Some(causes.unwrap_or(first).with_content(spacing));
        }
        causes.map_or(Ok(()), Err)
    }

    /// Open a functional channel.
    ///
    /// ``UDSS_LLR_0121`` and ``UDSS_LLR_0122`` as for a physical channel, the channel
    /// identified by the [`Ai`] `addressing` forms with [`TaType::Functional`]. Opening
    /// supplies this channel's parameters, per ``UDSS_LLR_0042``. ``UDSS_LLR_0139`` gives
    /// it a responder table of `R` entries. Rejected where no functional slot is free.
    ///
    /// `addressing` carries no `ta_type`: this method supplies
    /// [`TaType::Functional`] itself, so a channel opened here can never disagree with
    /// ``UDSS_LLR_0049`` by carrying [`TaType::Physical`].
    pub fn open_functional_channel(
        &mut self,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R, FunctionalChannelId> {
        let ai = addressing.with_ta_type(TaType::Functional);
        self.expire(now);
        let free = self.functional.iter().any(|s| s.channel.is_none());
        let outcome = self.validate_open(ai, free).map(|()| {
            let id = self.issue();
            if let Some(slot) = self.functional.iter_mut().find(|s| s.channel.is_none()) {
                slot.channel = Some(Channel::opened(id, ai, params));
                slot.responders = [ResponderSlot::EMPTY; R]; // UDSS_LLR_0142
            }
            FunctionalChannelId(id)
        });
        Reaction::new(self, [None, None], outcome)
    }

    /// Withdraw a channel, which discards it.
    ///
    /// ``UDSS_LLR_0125`` — permitted at any time, discarding without output every fact
    /// this set holds for the channel, an outstanding association included. It is the
    /// caller's last exit: a transmission whose confirmation never comes leaves its
    /// association outstanding and only withdrawal clears it. ``UDSS_LLR_0124`` rejects
    /// one naming a channel the client does not have.
    pub fn withdraw_channel(
        &mut self,
        now: Timestamp,
        channel: impl Into<ChannelId>,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        let channel = channel.into();
        self.expire(now);
        let physical = self.physical.iter_mut().find(|s| {
            s.channel
                .as_ref()
                .is_some_and(|p| p.core.handle() == channel)
        });
        let outcome = if let Some(slot) = physical {
            slot.channel = None; // UDSS_LLR_0125
            Ok(())
        } else if let Some(slot) = self
            .functional
            .iter_mut()
            .find(|s| s.channel.as_ref().is_some_and(|c| c.handle() == channel))
        {
            slot.channel = None; // UDSS_LLR_0125
            Ok(())
        } else {
            Err(NO_SUCH_CHANNEL) // UDSS_LLR_0124
        };
        Reaction::new(self, [None, None], outcome)
    }

    /// Set a physical channel's protocol parameter.
    ///
    /// ``UDSS_LLR_0043`` permits it at any time; ``UDSS_LLR_0134`` rejects a setting
    /// naming a channel the client does not have. Its wrong-kind limb needs no check
    /// here: `channel` is a [`PhysicalChannelId`], so this can never name a functional
    /// channel. A physical channel's `tS3_Client` is not among the parameters — see
    /// [`Client::set_physical_s3_client`], which exists only in physical keep-alive.
    pub fn set_physical_parameter(
        &mut self,
        now: Timestamp,
        channel: PhysicalChannelId,
        parameter: ChannelParameter,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        self.expire(now);
        let outcome = match self.channel_mut(channel.into()) {
            Some(c) => {
                c.set(parameter);
                Ok(())
            }
            None => Err(NO_SUCH_CHANNEL), // UDSS_LLR_0134
        };
        Reaction::new(self, [None, None], outcome)
    }

    /// Set a functional channel's protocol parameter.
    ///
    /// ``UDSS_LLR_0043`` permits it at any time; ``UDSS_LLR_0134`` rejects a setting
    /// naming a channel the client does not have. Its wrong-kind limb needs no check
    /// here: `channel` is a [`FunctionalChannelId`], so this can never name a physical
    /// channel.
    pub fn set_functional_parameter(
        &mut self,
        now: Timestamp,
        channel: FunctionalChannelId,
        parameter: ChannelParameter,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        self.expire(now);
        let outcome = match self.channel_mut(channel.into()) {
            Some(c) => {
                c.set(parameter);
                Ok(())
            }
            None => Err(NO_SUCH_CHANNEL), // UDSS_LLR_0134
        };
        Reaction::new(self, [None, None], outcome)
    }

    /// Reset a channel.
    ///
    /// ``UDSS_LLR_0180`` — ends any request in progress and stops its timer with no
    /// indication, marks an unconfirmed association abandoned, closes the open
    /// start-of-message or releases every responder entry, and zeroes the repeat count.
    /// It produces no output. ISO 14229-2:2021 9.7 Table 9 ends at the third transmission
    /// and says nothing of what the client concludes, so something the caller invokes has
    /// to clear the state that persists on its own. ``UDSS_LLR_0183`` rejects a reset
    /// naming no existing channel.
    pub fn reset_channel(
        &mut self,
        now: Timestamp,
        channel: impl Into<ChannelId>,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        let channel = channel.into();
        self.expire(now);
        let found = match channel {
            ChannelId::Physical(id) => self.physical_mut(id).map(|p| {
                p.som_open = false;
                p.core.reset();
            }),
            ChannelId::Functional(id) => self.functional_mut(id).map(|s| {
                s.responders = [ResponderSlot::EMPTY; R];
                if let Some(c) = s.channel.as_mut() {
                    c.reset();
                }
            }),
        };
        let outcome = found.ok_or(NO_SUCH_CHANNEL); // UDSS_LLR_0183
        Reaction::new(self, [None, None], outcome) // UDSS_LLR_0180: no output
    }

    /// Release a keep-alive.
    ///
    /// ``UDSS_LLR_0184`` — clears the session fact and stops the timer, in whichever mode
    /// applies to the named channel; in every other case it changes nothing, and it
    /// produces no output. Without it the physical fact would keep restarting a
    /// keep-alive for a server the application has given up. It is separate from
    /// [`Client::reset_channel`] because the two answer different situations.
    pub fn release_keep_alive(
        &mut self,
        now: Timestamp,
        channel: impl Into<ChannelId>,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        let channel = channel.into();
        self.expire(now);
        if !self.channels().any(|c| c.handle() == channel) {
            return Reaction::new(self, [None, None], Err(NO_SUCH_CHANNEL)); // UDSS_LLR_0184
        }
        self.keep_alive_on(now, channel, Event::Released);
        Reaction::new(self, [None, None], Ok(()))
    }

    /// Request transmission of a request.
    ///
    /// ``UDSS_LLR_0033``. ``UDSS_LLR_0123`` rejects an addressing naming no existing
    /// channel, ``UDSS_LLR_0061`` a request duplicating an outstanding association,
    /// ``UDSS_LLR_0171`` one on a channel whose spacing timer is running — with
    /// ``UDSS_LLR_0172``'s remaining time in the report — and ``UDSS_LLR_0177`` and
    /// ``UDSS_LLR_0178`` a third repeat or one sent while responses are still arriving.
    pub fn s_data_req<'d>(
        &mut self,
        now: Timestamp,
        ai: Ai,
        data: &'d [u8],
        class: ClientTx,
    ) -> ClientReaction<'_, 'd, K, PHYS, FUNC, R> {
        self.expire(now);
        if let Err(rejection) = self.validate_req(now, ai, class) {
            return Reaction::new(self, [None, None], Err(rejection)); // UDSS_LLR_0015
        }
        let transmit = self.channels_mut().find(|c| c.ai == ai).map(|c| {
            // UDSS_LLR_0059
            c.sent = Some(Sent {
                class,
                abandoned: false,
            });
            c.count(class); // UDSS_LLR_0176
            ClientOutput::Transmit {
                channel: c.handle(),
                ai,
                data,
            }
        });
        if let Some(ClientOutput::Transmit { channel, .. }) = transmit {
            self.keep_alive_on(now, channel, Event::Sent);
        }
        Reaction::new(self, [transmit, None], Ok(()))
    }

    /// A message has started arriving.
    ///
    /// ``UDSS_LLR_0023`` — addressing and no result. ``UDSS_LLR_0026`` requires the
    /// caller to identify the channel, because the session layer cannot: a response from
    /// one server may belong to the physical channel to that server or to a functional
    /// channel it was reached through, and nothing in the indication says which. A
    /// mandatory parameter discharges ``UDSS_LLR_0027``'s second limb by construction,
    /// as that requirement anticipates. ``UDSS_LLR_0028`` does not check it against the
    /// addressing.
    pub fn t_data_som_ind(
        &mut self,
        now: Timestamp,
        channel: impl Into<ChannelId>,
        ai: Ai,
        class: ClientRx,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        let channel = channel.into();
        self.expire(now);
        if let Err(rejection) = self.validate_ind(channel, SResult::Ok, Some(class)) {
            return Reaction::new(self, [None, None], Err(rejection)); // UDSS_LLR_0015
        }
        let capacity = match channel {
            ChannelId::Physical(id) => {
                if let Some(p) = self.physical_mut(id) {
                    p.on_som(class);
                }
                None
            }
            ChannelId::Functional(id) => self
                .functional_mut(id)
                .is_some_and(|s| s.on_som(now, ai.source(), class))
                .then(|| capacity(id, ai)),
        };
        Reaction::new(self, [capacity, None], Ok(())) // UDSS_LLR_0038
    }

    /// A message has finished arriving.
    ///
    /// ``UDSS_LLR_0036`` indicates it to the application; ``UDSS_LLR_0045`` pairs it with
    /// an open start-of-message; ``UDSS_LLR_0146`` records an outstanding
    /// response-pending message; ``UDSS_LLR_0143`` reports a responder beyond capacity.
    ///
    /// `class` is optional because ``UDSS_LLR_0058`` permits a `T_Data.ind` reporting an
    /// unsuccessful reception of a message not addressed to a server to omit the kind, and
    /// ``UDSS_LLR_0069`` makes that the only permitted omission, rejecting a `class` of
    /// `None` in every other case — including a *successful* reception, which is still a
    /// ``UDSS_LLR_0069`` rejection and not this permitted case. [`Cause::KindRequired`]
    /// reports it.
    ///
    /// [`Cause::KindRequired`]: crate::rejection::Cause::KindRequired
    pub fn t_data_ind<'d>(
        &mut self,
        now: Timestamp,
        channel: impl Into<ChannelId>,
        ai: Ai,
        data: &'d [u8],
        result: SResult,
        class: Option<ClientRx>,
    ) -> ClientReaction<'_, 'd, K, PHYS, FUNC, R> {
        let channel = channel.into();
        self.expire(now);
        if let Err(rejection) = self.validate_ind(channel, result, class) {
            return Reaction::new(self, [None, None], Err(rejection)); // UDSS_LLR_0015
        }
        let capacity = match channel {
            ChannelId::Physical(id) => {
                if let Some(p) = self.physical_mut(id) {
                    p.on_ind(now, result, class);
                }
                None
            }
            ChannelId::Functional(id) => self
                .functional_mut(id)
                .is_some_and(|s| s.on_ind(now, ai.source(), result, class))
                .then(|| capacity(id, ai)),
        };
        // UDSS_LLR_0036; UDSS_LLR_0143 puts the capacity indication first.
        let indicate = ClientOutput::Indicate {
            channel,
            ai,
            data,
            result,
        };
        let ok = result == SResult::Ok;
        self.keep_alive_on(now, channel, Event::Received { ok, class });
        Reaction::new(self, [capacity, Some(indicate)], Ok(()))
    }

    /// A transmission has completed.
    ///
    /// ``UDSS_LLR_0025``. It takes no channel, deliberately: ``UDSS_LLR_0059`` matches a
    /// confirmation to its association by addressing alone, which is the standard's own
    /// rule in ISO 14229-2:2021 7.6. ``UDSS_LLR_0063`` rejects one matching none, which
    /// is what a confirmation arriving after its channel was withdrawn does.
    pub fn t_data_conf(
        &mut self,
        now: Timestamp,
        ai: Ai,
        result: SResult,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        self.expire(now);
        let matched = self.channels_mut().find_map(|c| {
            let sent = c.sent.take_if(|_| c.ai == ai)?; // UDSS_LLR_0059
            Some((c, sent))
        });
        let Some((channel, Sent { class, abandoned })) = matched else {
            let rejection = Rejection::new(Cause::NoMatchingAssociation);
            return Reaction::new(self, [None, None], Err(rejection)); // UDSS_LLR_0063
        };
        let ok = result == SResult::Ok;
        let expects = class.expected() != ExpectedResponses::None;
        // UDSS_LLR_0169 on a physical channel, UDSS_LLR_0170 on a functional one
        if channel.ai.ta_type == TaType::Functional || !ok || !expects {
            channel.spacing.start(now, channel.params.spacing);
        }
        if ok && expects && !abandoned {
            // UDSS_LLR_0135, UDSS_LLR_0128; UDSS_LLR_0182
            channel.restart(now, ChannelReload::Default);
            channel.request = Some(InProgress { class, received: 0 });
        }
        let handle = channel.handle();
        self.keep_alive_on(now, handle, Event::Confirmed { ok, class }); // UDSS_LLR_0182
        Reaction::new(
            self,
            [Some(ClientOutput::Confirm { ai, result }), None],
            Ok(()),
        )
    }

    /// Supply a timestamp on its own.
    ///
    /// ``UDSS_LLR_0010`` — a timestamp accompanies every other input and is also supplied
    /// on its own; this is that input. ``UDSS_LLR_0079`` evaluates expiry only when a
    /// timestamp is supplied, so this is how a `tP_Client`, spacing or `tS3_Client` timer
    /// that has run out is noticed when nothing else is happening. Its only outputs are
    /// the indications of the expiries it causes.
    pub fn tick(
        &mut self,
        now: Timestamp,
    ) -> ClientReaction<'_, 'static, K, PHYS, FUNC, R> {
        self.expire(now);
        Reaction::new(self, [None, None], Ok(()))
    }

    /// The earliest timestamp at which a supplied timestamp could expire a timer.
    ///
    /// ``UDSS_LLR_0080`` — `None` where no timer is running; otherwise the earliest
    /// deadline among every channel's response and spacing timers, each physical
    /// channel's `tS3_Client` in physical keep-alive, and the client-wide `tS3_Client` of
    /// ``UDSS_LLR_0150`` in functional keep-alive, chosen across the timestamp wrap
    /// (``UDSS_LLR_0019``). `tP_Client` expires once its window is exceeded
    /// (``UDSS_LLR_0148``), so its deadline is one millisecond past the window: a caller
    /// supplying exactly this timestamp finds the expiry. Like the server's, this is a
    /// query, not an output, and produces no reaction.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let physical = self
            .physical
            .iter()
            .filter_map(|s| s.channel.as_ref())
            .map(|p| K::channel_deadline(&p.keep_alive));
        self.channels()
            .flat_map(Channel::deadlines)
            .chain(physical)
            .chain([self.keep_alive.deadline()])
            .flatten()
            .reduce(earlier)
    }
}

impl<const PHYS: usize, const FUNC: usize, const R: usize>
    Client<FunctionalKeepAlive, PHYS, FUNC, R>
{
    /// Open a physical channel.
    ///
    /// ``UDSS_LLR_0121`` — the channel exists from this moment, identified by the [`Ai`]
    /// `addressing` forms with [`TaType::Physical`], until it is withdrawn.
    /// ``UDSS_LLR_0122`` rejects an addressing equal to an existing channel's. Opening
    /// supplies this channel's parameters, per ``UDSS_LLR_0042``. Rejected where no
    /// physical slot is free.
    ///
    /// `addressing` carries no `ta_type`: this method supplies [`TaType::Physical`]
    /// itself, so a channel opened here can never disagree with ``UDSS_LLR_0049`` by
    /// carrying [`TaType::Functional`].
    ///
    /// There is no `tS3_Client` argument. ``UDSS_LLR_0152`` gives a physical channel no
    /// reload in functional keep-alive, and this method is the one reachable in that mode,
    /// so the first of that requirement's three disagreements cannot be written.
    pub fn open_physical_channel(
        &mut self,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
    ) -> ClientReaction<'_, 'static, FunctionalKeepAlive, PHYS, FUNC, R, PhysicalChannelId>
    {
        self.open_physical(now, addressing, params, ())
    }

    /// Set the client-wide `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0040`` puts protocol-parameter setting in the service interface;
    /// ``UDSS_LLR_0042`` gives this reload no default and ``UDSS_LLR_0152`` gives
    /// functional keep-alive exactly one; ``UDSS_LLR_0043`` permits setting it again at
    /// any time.
    ///
    /// It exists only here. ``UDSS_LLR_0152`` has no client-wide reload in physical
    /// keep-alive, and a method absent from that mode is the third of that requirement's
    /// disagreements made unwritable.
    pub fn set_keep_alive_reload(
        &mut self,
        now: Timestamp,
        s3_client: u32,
    ) -> ClientReaction<'_, 'static, FunctionalKeepAlive, PHYS, FUNC, R> {
        self.expire(now);
        self.keep_alive.reload = s3_client; // UDSS_LLR_0152
        Reaction::new(self, [None, None], Ok(()))
    }
}

impl<const PHYS: usize, const FUNC: usize, const R: usize>
    Client<PhysicalKeepAlive, PHYS, FUNC, R>
{
    /// Open a physical channel, with this channel's `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0121``, ``UDSS_LLR_0122`` and ``UDSS_LLR_0042`` as in functional
    /// keep-alive — see [`Client::open_physical_channel`] on that mode, whose signature
    /// differs from this one only in the absent reload.
    ///
    /// `s3_client` is required, not optional. ``UDSS_LLR_0151`` puts the timer and session
    /// fact in this channel's storage and ``UDSS_LLR_0152`` gives the channel its own
    /// reload in this mode, so omitting it is the second of that requirement's
    /// disagreements — and a mandatory argument is what makes it unwritable.
    pub fn open_physical_channel(
        &mut self,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
        s3_client: u32,
    ) -> ClientReaction<'_, 'static, PhysicalKeepAlive, PHYS, FUNC, R, PhysicalChannelId>
    {
        let session = PhysicalSession::new(s3_client); // UDSS_LLR_0151, UDSS_LLR_0152
        self.open_physical(now, addressing, params, session)
    }

    /// Set a physical channel's `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0043`` permits it at any time; ``UDSS_LLR_0134`` rejects a setting
    /// naming a channel the client does not have. It is separate from
    /// [`Client::set_physical_parameter`] because ``UDSS_LLR_0152`` gives a physical
    /// channel a reload in this mode alone, and a parameter absent from the other mode's
    /// surface is what keeps the first of that requirement's disagreements unwritable.
    pub fn set_physical_s3_client(
        &mut self,
        now: Timestamp,
        channel: PhysicalChannelId,
        s3_client: u32,
    ) -> ClientReaction<'_, 'static, PhysicalKeepAlive, PHYS, FUNC, R> {
        self.expire(now);
        let outcome = match self.physical_mut(channel) {
            Some(p) => {
                p.keep_alive.reload = s3_client; // UDSS_LLR_0152
                Ok(())
            }
            None => Err(NO_SUCH_CHANNEL), // UDSS_LLR_0134
        };
        Reaction::new(self, [None, None], outcome)
    }
}

impl<K: KeepAliveMode, const PHYS: usize, const FUNC: usize, const R: usize>
    crate::sealed::Sealed for Client<K, PHYS, FUNC, R>
{
}

impl<'d, K: KeepAliveMode, const PHYS: usize, const FUNC: usize, const R: usize>
    crate::reaction::Drain<'d, ClientOutput<'d>> for Client<K, PHYS, FUNC, R>
{
    fn next_expiry(&mut self) -> Option<ClientOutput<'d>> {
        let timeout = |t: TimedOut| ClientOutput::ResponseTimeout {
            ai: t.ai,
            loaded: t.loaded,
        };
        let physical = self.physical.iter_mut().find_map(|s| {
            s.timed_out.take().map(timeout).or_else(|| {
                let channel = s.keep_alive_due.take()?;
                Some(ClientOutput::KeepAliveDue {
                    channel: Some(channel),
                })
            })
        });
        let functional = || {
            self.functional
                .iter_mut()
                .find_map(|s| s.timed_out.take().map(timeout))
        };
        physical.or_else(functional).or_else(|| {
            core::mem::take(&mut self.keep_alive_due)
                .then_some(ClientOutput::KeepAliveDue { channel: None })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Client, PhysicalSlot};
    use crate::addressing::{Address, ChannelAddressing, Mtype};
    use crate::keep_alive::FunctionalKeepAlive;
    use crate::params::{ChannelParams, Reloads};
    use crate::rejection::Cause;
    use crate::time::Timestamp;

    /// ``UDSS_LLR_0185`` (second limb) — once the last handle is issued, no open
    /// succeeds, even with a slot free, and the report says the handles are spent, not
    /// the slots.
    #[test]
    fn an_open_with_every_handle_issued_is_rejected() {
        let mut c: Client<FunctionalKeepAlive, 1, 0> =
            Client::new([PhysicalSlot::EMPTY], [], FunctionalKeepAlive::new(2_000));
        c.next_id = u32::MAX;
        let addressing = ChannelAddressing {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0010),
        };
        let params = ChannelParams {
            reloads: Reloads {
                default_reload: 50,
                enhanced_reload: 5_000,
            },
            spacing: 60,
        };
        let r = c
            .open_physical_channel(Timestamp(0), addressing, params)
            .finish();
        assert!(r.is_err_and(|e| e.contains(Cause::ChannelHandlesSpent)
            && !e.contains(Cause::NoChannelSlotFree)));
    }
}
