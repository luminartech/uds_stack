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
//! `R` sizes every functional channel's responder table alike; a client with no
//! functional channels at all sets `FUNC` to `0` and pays no storage for one.
//!
//! ```
//! use uds_session::{
//!     Client, FunctionalKeepAlive, FunctionalSlot, KeepAliveMode, PhysicalSlot,
//! };
//!
//! let client: Client<4, 1, 8> = Client::new(
//!     [PhysicalSlot::EMPTY; 4],
//!     [FunctionalSlot::EMPTY; 1],
//!     KeepAliveMode::Functional {
//!         storage: FunctionalKeepAlive::EMPTY,
//!         s3_client: 2_000,
//!     },
//! );
//! # let _ = client;
//! ```

use crate::addressing::{Address, AddressExtension, Ai, ChannelAddressing, TaType};
use crate::classification::{ClientRx, ClientTx};
use crate::params::{
    ChannelReload, FunctionalChannelParameter, FunctionalChannelParams,
    PhysicalChannelParameter, PhysicalChannelParams,
};
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
/// Operations that act on either kind take this; those that act on one take that kind's
/// own identity, which is why no setting can name a channel of the wrong kind.
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
    /// ``UDSS_LLR_0121`` — the same channel, named by the identity a kind-agnostic
    /// method such as [`Client::withdraw_channel`] takes.
    fn from(id: PhysicalChannelId) -> Self {
        Self::Physical(id)
    }
}

impl From<FunctionalChannelId> for ChannelId {
    /// Widen a functional channel's identity to either kind's.
    ///
    /// ``UDSS_LLR_0121`` — the same channel, named by the identity a kind-agnostic
    /// method such as [`Client::withdraw_channel`] takes.
    fn from(id: FunctionalChannelId) -> Self {
        Self::Functional(id)
    }
}

/// The client-wide keep-alive state of functional mode.
///
/// ``UDSS_LLR_0150`` — one `tS3_Client` timer and one keeping-alive fact, fixed in size
/// and nonetheless caller-supplied, so that one storage shape serves both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionalKeepAlive {
    _reserved: (),
}

impl FunctionalKeepAlive {
    /// The initial state: no session kept alive and the timer not running.
    ///
    /// ``UDSS_LLR_0153`` — the client's session timer is initially not running.
    pub const EMPTY: Self = Self { _reserved: () };
}

/// How the client keeps servers alive.
///
/// ``UDSS_LLR_0149`` — fixed when the instance is created; no input changes it. The mode
/// selects which of ``UDSS_LLR_0155`` to ``UDSS_LLR_0163`` and ``UDSS_LLR_0184`` act.
#[derive(Debug, PartialEq, Eq)]
pub enum KeepAliveMode {
    /// A functionally addressed `TesterPresent` each time the client's `tS3_Client`
    /// expires. ISO 14229-2:2021 9.6 Table 8 allots a single timer here.
    ///
    /// ``UDSS_LLR_0150`` — the client-wide timer and fact, supplied by value with the
    /// instance, and the single `tS3_Client` reload of ``UDSS_LLR_0152``.
    Functional {
        /// The client-wide timer and keeping-alive fact.
        storage: FunctionalKeepAlive,
        /// The single `tS3_Client` reload, which must cover the longest path among every
        /// server the functional address reaches.
        s3_client: u32,
    },
    /// A physically addressed `TesterPresent` on a physical channel when that channel's
    /// `tS3_Client` expires with no other request sent on it.
    ///
    /// ``UDSS_LLR_0151`` — the fact and timer live in each physical channel's storage and
    /// ``UDSS_LLR_0152`` puts that channel's reload in
    /// [`crate::PhysicalChannelParams::s3_client`], so this variant carries nothing.
    Physical,
}

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
#[derive(Debug)]
pub struct Client<const PHYS: usize, const FUNC: usize, const R: usize> {
    _physical: [PhysicalSlot; PHYS],
    _functional: [FunctionalSlot<R>; FUNC],
    _keep_alive: KeepAliveMode,
}

impl<const PHYS: usize, const FUNC: usize, const R: usize> Client<PHYS, FUNC, R> {
    /// Create a client.
    ///
    /// ``UDSS_LLR_0032`` — creation supplies the keep-alive mode of ``UDSS_LLR_0149``
    /// and, in functional keep-alive, the storage of ``UDSS_LLR_0150`` and the reload of
    /// ``UDSS_LLR_0152``. Channel storage is supplied here too, by value: `PHYS` physical
    /// slots and `FUNC` functional slots of `R` responders each. A channel begins to
    /// exist when the caller opens one, under ``UDSS_LLR_0121``.
    #[must_use]
    pub const fn new(
        physical: [PhysicalSlot; PHYS],
        functional: [FunctionalSlot<R>; FUNC],
        keep_alive: KeepAliveMode,
    ) -> Self {
        Self {
            _physical: physical,
            _functional: functional,
            _keep_alive: keep_alive,
        }
    }

    /// Open a physical channel.
    ///
    /// ``UDSS_LLR_0121`` — the channel exists from this moment, identified by the [`Ai`]
    /// `addressing` forms with [`TaType::Physical`], until it is withdrawn.
    /// ``UDSS_LLR_0122`` rejects an addressing equal to an existing channel's. Opening
    /// supplies this channel's parameters, per ``UDSS_LLR_0042``. Rejected where no
    /// physical slot is free, and rejected under [`crate::Cause::S3ClientReloadMismatch`]
    /// where ``params.s3_client`` disagrees with the client's keep-alive mode, as
    /// ``UDSS_LLR_0152`` requires.
    ///
    /// `addressing` carries no `ta_type`: this method supplies
    /// [`TaType::Physical`] itself, so a channel opened here can never disagree with
    /// ``UDSS_LLR_0049`` by carrying [`TaType::Functional`].
    pub fn open_physical_channel(
        &mut self,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: PhysicalChannelParams,
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
        params: FunctionalChannelParams,
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
        channel: ChannelId,
    ) -> ClientReaction<'_, 'static> {
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
    /// channel. Setting [`PhysicalChannelParameter::S3Client`] to a value while the client
    /// is in functional keep-alive, where the channel's reload has no meaning, is rejected
    /// under [`crate::Cause::S3ClientReloadMismatch`], as ``UDSS_LLR_0152`` requires.
    pub fn set_physical_parameter(
        &mut self,
        now: Timestamp,
        channel: PhysicalChannelId,
        parameter: PhysicalChannelParameter,
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
        parameter: FunctionalChannelParameter,
    ) -> ClientReaction<'_, 'static> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!("UDSS_LLR_0043, 0134: {now:?} {channel:?} {parameter:?}")
        }
    }

    /// Set the client-wide `tS3_Client` reload of functional keep-alive.
    ///
    /// ``UDSS_LLR_0040`` puts protocol-parameter setting in the service interface;
    /// ``UDSS_LLR_0042`` gives this reload no default, ``UDSS_LLR_0152`` its one supply
    /// point per mode; ``UDSS_LLR_0043`` permits setting it again at any time. In physical
    /// keep-alive there is no client-wide reload to set, so the call is rejected under
    /// [`crate::Cause::S3ClientReloadMismatch`], as ``UDSS_LLR_0152`` requires: the same
    /// cause as a physical channel's own mismatch, since both are a reload disagreeing
    /// with the client's keep-alive mode.
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
        channel: ChannelId,
    ) -> ClientReaction<'_, 'static> {
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
        channel: ChannelId,
    ) -> ClientReaction<'_, 'static> {
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
        channel: ChannelId,
        ai: Ai,
        class: ClientRx,
    ) -> ClientReaction<'_, 'static> {
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
        channel: ChannelId,
        ai: Ai,
        data: &'d [u8],
        result: SResult,
        class: Option<ClientRx>,
    ) -> ClientReaction<'_, 'd> {
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
