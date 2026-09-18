//! The client role.
//!
//! ``UDSS_LLR_0029`` fixes the role at creation; see [`crate::server`] for why the two
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
//! `Client<2, 0>`.
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

use crate::addressing::{Address, AddressExtension, Ai, ChannelAddressing, TaType};
use crate::classification::{ClientRx, ClientTx};
use crate::params::{ChannelParameter, ChannelParams, ChannelReload};
use crate::reaction::Reaction;
use crate::result::SResult;
use crate::time::Timestamp;

/// One entry of a functional channel's responder table.
///
/// ``UDSS_LLR_0139`` — keyed by a responder's peer identity, recording whether a
/// start-of-message is open and whether a response-pending message is outstanding.
/// ISO 14229-2:2021 9.6 Table 7 allots the client one timer per channel and no storage
/// for either fact, which is why this requirement is derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponderSlot {
    _reserved: (),
}

impl ResponderSlot {
    /// A free entry.
    pub const EMPTY: Self = Self { _reserved: () };
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
#[derive(Debug)]
pub struct PhysicalSlot {
    _reserved: (),
}

impl PhysicalSlot {
    /// A slot holding no channel.
    pub const EMPTY: Self = Self { _reserved: () };
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
    _responders: [ResponderSlot; R],
}

impl<const R: usize> FunctionalSlot<R> {
    /// A slot holding no channel, its responder table empty.
    ///
    /// ``UDSS_LLR_0142`` — a channel's responder table holds no entry when it is opened.
    pub const EMPTY: Self = Self {
        _responders: [ResponderSlot::EMPTY; R],
    };
}

/// Identifies a physical channel of this client.
///
/// ``UDSS_LLR_0121`` — returned when the caller opens one, valid until it is withdrawn.
/// `PHYS` indexes from zero independently of `FUNC`, so this carries no ordering across
/// the two arrays; `PartialOrd` and `Ord` are not derived, since nothing needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalChannelId(u16);

/// Identifies a functional channel of this client.
///
/// ``UDSS_LLR_0121`` — returned when the caller opens one, valid until it is withdrawn.
/// `FUNC` indexes from zero independently of `PHYS`, so this carries no ordering across
/// the two arrays; `PartialOrd` and `Ord` are not derived, since nothing needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionalChannelId(u16);

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

mod sealed {
    /// Sealed so that the keep-alive modes are exactly the two ``UDSS_LLR_0149`` names.
    pub trait Sealed {}
}

/// How the client keeps servers alive.
///
/// ``UDSS_LLR_0149`` — one of two modes, fixed when the instance is created, changed by no
/// input. The mode selects which state ``UDSS_LLR_0150`` or ``UDSS_LLR_0151`` requires and
/// which of ``UDSS_LLR_0155`` to ``UDSS_LLR_0163`` and ``UDSS_LLR_0184`` act.
///
/// It is a type parameter of [`Client`] rather than a value inside it because
/// ``UDSS_LLR_0149`` settles it at creation and nothing afterwards can move it. Holding it
/// in the type is what lets ``UDSS_LLR_0152`` be satisfied without a check: the methods
/// that supply a `tS3_Client` reload exist only on the mode that gives one a meaning, so
/// none of that requirement's three disagreements can be written.
///
/// The trait is sealed. A mode is not an extension point — the standard names two — and
/// ``UDSS_LLR_0011`` forbids the session layer to deliver an output through a
/// caller-supplied trait implementation, which sealing keeps true of every trait here.
pub trait KeepAlive: sealed::Sealed + core::fmt::Debug {}

/// Functional keep-alive: one `TesterPresent` for the client, functionally addressed.
///
/// ``UDSS_LLR_0150`` — a single `tS3_Client` timer and a single keeping-alive fact for the
/// instance, with the single reload of ``UDSS_LLR_0152``. ISO 14229-2:2021 9.6 Table 8
/// allots one timer here, so this value is fixed in size; it is caller-supplied all the
/// same, because ``UDSS_LLR_0008`` puts every fact the client holds in the caller's
/// storage and a fact with nothing left to size is no exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionalKeepAlive {
    _s3_client: u32,
}

impl FunctionalKeepAlive {
    /// The initial state, with the client-wide `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0153`` — no session is kept alive and the timer is not running.
    /// ``UDSS_LLR_0152`` — `s3_client` must cover the longest path among every server the
    /// functional address reaches. [`Client::set_keep_alive_reload`] sets it again.
    #[must_use]
    pub const fn new(s3_client: u32) -> Self {
        Self {
            _s3_client: s3_client,
        }
    }
}

impl sealed::Sealed for FunctionalKeepAlive {}
impl KeepAlive for FunctionalKeepAlive {}

/// Physical keep-alive: a `TesterPresent` per physical channel, physically addressed.
///
/// ``UDSS_LLR_0151`` — the timer and session fact live in each physical channel's own
/// storage, and ``UDSS_LLR_0152`` gives each physical channel its own reload, supplied at
/// [`Client::open_physical_channel`]. Nothing is client-wide, so this mode carries no
/// value at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalKeepAlive;

impl sealed::Sealed for PhysicalKeepAlive {}
impl KeepAlive for PhysicalKeepAlive {}

/// What a client produces for the caller to retrieve.
///
/// ``UDSS_LLR_0012`` — the standard's own outputs, plus the ones it does not define.
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
pub type ClientReaction<'s, 'd, T = ()> = Reaction<'s, 'd, ClientOutput<'d>, T>;

/// The session layer in the client role.
///
/// ``UDSS_LLR_0008`` — all state lives here: the channel storage and keep-alive mode are
/// supplied by the caller at creation, and opening a channel supplies that channel's own
/// parameters. The arrays split by channel kind because the two kinds hold different
/// state: ``UDSS_LLR_0139`` gives a responder table to functional channels alone, and
/// ``UDSS_LLR_0151`` a `tS3_Client` to physical ones alone.
///
/// `K` is the keep-alive mode of ``UDSS_LLR_0149``, fixed at creation and held in the type
/// rather than in a field — see [`KeepAlive`]. It is inferred from the value passed to
/// [`Client::new`], so a caller names it only where they annotate the type.
///
/// `R` defaults to `0`, since a client with `FUNC` of `0` has no responder table to size.
#[derive(Debug)]
pub struct Client<K: KeepAlive, const PHYS: usize, const FUNC: usize, const R: usize = 0> {
    _physical: [PhysicalSlot; PHYS],
    _functional: [FunctionalSlot<R>; FUNC],
    _keep_alive: K,
}

impl<K: KeepAlive, const PHYS: usize, const FUNC: usize, const R: usize>
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
        physical: [PhysicalSlot; PHYS],
        functional: [FunctionalSlot<R>; FUNC],
        keep_alive: K,
    ) -> Self {
        Self {
            _physical: physical,
            _functional: functional,
            _keep_alive: keep_alive,
        }
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
    ) -> ClientReaction<'_, 'static, FunctionalChannelId> {
        let ai = addressing.with_ta_type(TaType::Functional);
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0121, 0122, 0139, 0042: {now:?} {ai:?} {params:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        let channel = channel.into();
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0124, 0125: {now:?} {channel:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0043, 0134: {now:?} {channel:?} {parameter:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0043, 0134: {now:?} {channel:?} {parameter:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        let channel = channel.into();
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0180, 0183: {now:?} {channel:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        let channel = channel.into();
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0184: {now:?} {channel:?}")
        }
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
    ) -> ClientReaction<'_, 'd> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!(
                "UDSS_LLR_0033, 0123: {now:?} {ai:?} {} {class:?}",
                data.len()
            )
        }
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
    ) -> ClientReaction<'_, 'static> {
        let channel = channel.into();
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0023, 0026, 0136: {now:?} {channel:?} {ai:?} {class:?}")
        }
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
    ) -> ClientReaction<'_, 'd> {
        let channel = channel.into();
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!(
                "UDSS_LLR_0036, 0045, 0058, 0069: {now:?} {channel:?} {ai:?} {} {result:?} \
                 {class:?}",
                data.len()
            )
        }
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
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0025, 0059, 0063: {now:?} {ai:?} {result:?}")
        }
    }

    /// Supply a timestamp on its own.
    ///
    /// ``UDSS_LLR_0010`` and ``UDSS_LLR_0079`` — see [`crate::Server::tick`].
    pub fn tick(&mut self, now: Timestamp) -> ClientReaction<'_, 'static> {
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
    /// ``UDSS_LLR_0080`` — see [`crate::Server::next_deadline`]: the earliest deadline of
    /// any timer currently running, on a client the response, spacing and session timers
    /// of every channel, and, in functional keep-alive, the client-wide `tS3_Client` of
    /// ``UDSS_LLR_0150``, which belongs to no channel and which ``UDSS_LLR_0156`` expires.
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
    ) -> ClientReaction<'_, 'static, PhysicalChannelId> {
        let ai = addressing.with_ta_type(TaType::Physical);
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0121, 0122, 0042: {now:?} {ai:?} {params:?}")
        }
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
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0040, 0042, 0043, 0152: {now:?} {s3_client:?}")
        }
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
    ) -> ClientReaction<'_, 'static, PhysicalChannelId> {
        let ai = addressing.with_ta_type(TaType::Physical);
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!(
                "UDSS_LLR_0121, 0122, 0042, 0152: {now:?} {ai:?} {params:?} {s3_client:?}"
            )
        }
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
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0043, 0134, 0152: {now:?} {channel:?} {s3_client:?}")
        }
    }
}
