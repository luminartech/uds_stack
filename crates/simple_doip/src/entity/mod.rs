//! [`Entity`]: a `DoIP` entity's `TCP_DATA` sockets over an `edge-nal` acceptor, as a
//! [`DiagnosticEntity`].
//!
//! The entity accepts connections, activates routing and checks that registered
//! testers are alive by itself, inside [`DiagnosticEntity::next_event`], following
//! ISO 13400-2:2019 Figures 16, 17, 22 and 25 to 28, and keeps the three `TCP_DATA`
//! timers of Table 12.
//!
//! It takes headers of ISO 13400-2:2012 and 2019 and answers each in the version it
//! received; any other protocol version, the 2010 draft's included, is refused with
//! [`IncorrectPatternFormat`](crate::messages::NackCode::IncorrectPatternFormat) and
//! the socket closed.
//!
//! It activates routing for the two activation types Table 47 makes mandatory,
//! [`ActivationTypeCode::Default`] and [`ActivationTypeCode::RegulationRequired`], and
//! refuses every other with
//! [`RoutingActivationResponseCode::DeniedUnsupportedRoutingActivationType`].

mod handler;
mod io;
mod outbox;
mod table;

use core::fmt;

use core::future::{Future, pending, poll_fn};
use core::pin::pin;
use core::task::{Context, Poll};
use edge_nal::TcpAccept;

use embassy_futures::select::{Either, select, select_array};
use embassy_time::{Duration, Instant, Timer};

use crate::messages::{
    ActivationTypeCode, Message, ProtocolVersion, RoutingActivationResponseCode,
};
use crate::service::{
    ConnectionId, DiagnosticEntity, DoIpResult, EntityConfig, EntityEvent, Refusal,
    Timestamp,
};
use crate::stream::tx::Full;
use crate::stream::{after, caller_deadline, timestamp};
use crate::tester::DIAGNOSTIC_MESSAGE_OVERHEAD;
use crate::{LogicalAddress, TaType};
use handler::{ALIVE_CHECK_REQUEST, Handled, Limits};
use io::{Io, drive};
use table::{
    Activation, AliveCheck, AliveCheckScope, Arbitration, Phase, Slot, SlotRef, Stage,
};

/// The reserve socket's buffers: a routing activation request or response, with room
/// to spare for a header NACK.
const RESERVE_CAP: usize = 32;

/// How many [`DiagnosticEntity::request`]s may await their confirm at once.
const CONFIRMS: usize = 4;

/// `T_TCP_Initial_Inactivity` (ISO 13400-2:2019 Table 12).
const INITIAL_INACTIVITY: core::time::Duration = crate::TCP_TIMEOUT_INITIAL_INACTIVITY;
/// `T_TCP_General_Inactivity` (ISO 13400-2:2019 Table 12).
const GENERAL_INACTIVITY: core::time::Duration = crate::TCP_TIMEOUT_GENERAL_INACTIVITY;
/// `T_TCP_Alive_Check` (ISO 13400-2:2019 Table 12).
const ALIVE_CHECK: core::time::Duration = crate::TCP_TIMEOUT_ALIVE_CHECK;
/// How long an orderly close may take before the entity aborts it, discarding whatever
/// the tester has not yet acknowledged. ISO 13400-2 does not bound the close; this is
/// long enough for what was written before it to be retransmitted.
pub const ORDERLY_CLOSE_LIMIT: core::time::Duration = core::time::Duration::from_secs(2);
/// How long an abort may take before the socket is dropped.
const ABORT_LIMIT: core::time::Duration = ALIVE_CHECK;

/// When `timer`, started at `now`, expires: the end of time if that is past it.
fn expiry(now: Instant, timer: core::time::Duration) -> Instant {
    Duration::try_from(timer).map_or(Instant::MAX, |timer| after(now, timer))
}

/// The logical addresses an [`Entity`] answers diagnostic messages on.
///
/// # Examples
///
/// ```
/// use simple_doip::LogicalAddress;
/// use simple_doip::entity::EntityAddress;
///
/// let address = EntityAddress::new(LogicalAddress(0x0010), LogicalAddress(0xE400))?;
/// assert_eq!(address.physical(), LogicalAddress(0x0010));
/// # Ok::<(), simple_doip::entity::AddressError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityAddress {
    physical: LogicalAddress,
    functional: LogicalAddress,
}

/// Why an [`EntityAddress`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AddressError {
    /// The physical address is a functional or a tester address.
    #[error("{0} is not a physical address an entity can take")]
    NotPhysical(LogicalAddress),
    /// The functional address is outside the functional ranges.
    #[error("{0} is not a functional address")]
    NotFunctional(LogicalAddress),
}

impl EntityAddress {
    /// The entity's physical address and the one functional address it also answers.
    ///
    /// # Arguments
    ///
    /// * `physical` - the entity's own address: one whose
    ///   [`LogicalAddress::default_ta_type`] is [`TaType::Physical`], outside
    ///   [`LogicalAddress::MIN_CLIENT_ADDRESS`]..=[`LogicalAddress::MAX_CLIENT_ADDRESS`].
    /// * `functional` - the group address the entity belongs to: one whose
    ///   [`LogicalAddress::default_ta_type`] is [`TaType::Functional`].
    ///
    /// # Errors
    ///
    /// - [`AddressError::NotPhysical`] where `physical` is not a physical entity address.
    /// - [`AddressError::NotFunctional`] where `functional` is not a functional address.
    pub fn new(
        physical: LogicalAddress,
        functional: LogicalAddress,
    ) -> Result<Self, AddressError> {
        if physical.default_ta_type() != TaType::Physical
            || physical.is_valid_client_address()
        {
            return Err(AddressError::NotPhysical(physical));
        }
        if functional.default_ta_type() != TaType::Functional {
            return Err(AddressError::NotFunctional(functional));
        }
        Ok(Self {
            physical,
            functional,
        })
    }

    /// The entity's own address, the source of everything it sends.
    #[must_use]
    pub const fn physical(&self) -> LogicalAddress {
        self.physical
    }

    /// The functional group address the entity also answers.
    #[must_use]
    pub const fn functional(&self) -> LogicalAddress {
        self.functional
    }

    /// How a diagnostic message to `ta` addresses this entity, if it does.
    fn ta_type_of(self, ta: LogicalAddress) -> Option<TaType> {
        if ta == self.physical {
            Some(TaType::Physical)
        } else if ta == self.functional {
            Some(TaType::Functional)
        } else {
            None
        }
    }
}

/// Why an [`Entity`] failed.
///
/// `E` is the acceptor's error, [`TcpAccept::Error`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E> {
    /// Accepting a connection failed. Every call after this may fail the same way.
    #[error("accepting a connection failed: {0:?}")]
    Accept(E),
}

/// A `DoIP` entity: the `TCP_DATA` sockets of ISO 13400-2:2019 12.6, behind
/// [`DiagnosticEntity`].
///
/// - `MCTS` is the maximum number of concurrent `TCP_DATA` sockets (Table 11), at least
///   1 and at most 255. One more socket, the reserve, takes a further connection while
///   all `MCTS` are in use, as the socket handler requires.
/// - `MAX_MESSAGE` is the largest `DoIP` message, generic header included, the entity
///   receives or sends on a registered connection. Each of the `MCTS` connections holds
///   one receive buffer and one transmit queue of that size, for the diagnostic messages
///   it sends, and a small one apart for its own acknowledgements and other control
///   frames; the reserve holds small ones only.
/// - `TESTERS` is [`EntityConfig`]'s count of tester addresses.
///
/// The acceptor is borrowed for `'a` because every socket it accepts borrows it; bind
/// it first with [`edge_nal::TcpBind::bind`]. A connection beyond the `MCTS + 1` the
/// entity holds is accepted and dropped.
///
/// [`ConnectionId`]s are the connection slots' indices, below `MCTS`.
///
/// # Memory
///
/// The connections' buffers are most of an `Entity`: `2 × MCTS × MAX_MESSAGE` bytes.
/// Each slot's control queue, the reserve's buffers and the bookkeeping add a few
/// hundred bytes more, and
/// each of the `MCTS + 1` slots holds an accepted socket, [`TcpAccept::Socket`], inline
/// beside about a hundred bytes of its own. With `MCTS` 1, `MAX_MESSAGE` 4096 and
/// `edge-nal-std`'s sockets an `Entity` is about 8.7 KiB.
///
/// # Examples
///
/// An entity on the host, echoing every diagnostic message back to its sender:
///
/// ```no_run
/// use edge_nal::TcpBind;
/// use simple_doip::entity::{Entity, EntityAddress};
/// use simple_doip::service::{DiagnosticEntity, EntityConfig, EntityEvent, TesterAddress};
/// use simple_doip::{LogicalAddress, TCP_PORT, TaType};
///
/// # async fn serve() -> anyhow::Result<()> {
/// let stack = edge_nal_std::Stack::new();
/// let acceptor = stack.bind(([0, 0, 0, 0], TCP_PORT).into()).await?;
/// let address = EntityAddress::new(LogicalAddress(0x0001), LogicalAddress(0xE400))?;
/// let config = EntityConfig::new([TesterAddress::new(LogicalAddress(0x0E00))?]);
/// let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address, config);
/// let mut buf = [0u8; 4096];
/// loop {
///     if let EntityEvent::Indication { sa, pdu, .. } = entity.next_event(&mut buf, None).await? {
///         entity
///             .request(address.physical(), sa, TaType::Physical, pdu)
///             .await?;
///     }
/// }
/// # }
/// ```
///
/// # Cancel safety
///
/// [`DiagnosticEntity::next_event`], [`DiagnosticEntity::request`] and
/// [`DiagnosticEntity::close`] may each be dropped at any await and called again,
/// provided the acceptor and its sockets meet the crate's `connection` feature
/// conditions.
pub struct Entity<
    'a,
    A: TcpAccept + 'a,
    const MCTS: usize,
    const MAX_MESSAGE: usize,
    const TESTERS: usize = 1,
> {
    acceptor: &'a A,
    address: EntityAddress,
    config: EntityConfig<TESTERS>,
    connections: [Slot<A::Socket<'a>, MAX_MESSAGE>; MCTS],
    reserve: Slot<A::Socket<'a>, RESERVE_CAP>,
    arbitration: Option<Arbitration>,
    /// Oldest first.
    confirms: [Option<PendingConfirm>; CONFIRMS],
    /// Which source [`Self::wait`] polls first.
    turn: Turn,
}

/// A request awaiting its [`EntityEvent::Confirm`].
#[derive(Clone, Copy)]
struct PendingConfirm {
    connection: usize,
    end: u64,
    sa: LogicalAddress,
    ta: LogicalAddress,
    ta_type: TaType,
    result: Option<DoIpResult>,
}

/// A diagnostic message whose data [`handler::handle`] has copied into the caller's
/// buffer.
struct Delivery {
    connection: ConnectionId,
    sa: LogicalAddress,
    ta: LogicalAddress,
    ta_type: TaType,
    copied: usize,
    length: usize,
}

impl Delivery {
    fn into_event(self, buf: &[u8]) -> EntityEvent<'_> {
        let Self {
            connection,
            sa,
            ta,
            ta_type,
            copied,
            length,
        } = self;
        let pdu = buf.get(..copied).unwrap_or_default();
        if copied == length {
            EntityEvent::Indication {
                connection,
                sa,
                ta,
                ta_type,
                pdu,
            }
        } else {
            EntityEvent::IndicationTruncated {
                connection,
                sa,
                ta,
                ta_type,
                pdu,
                length,
            }
        }
    }
}

/// What handling a buffered frame came to.
enum Step {
    Idle,
    Progress,
    Deliver(Delivery),
}

/// The source [`Entity::wait`] polls first, after the one that won last.
#[derive(Clone, Copy)]
enum Turn {
    Connection(usize),
    Reserve,
    Acceptor,
}

/// The three sources [`Entity::wait`] polls, in turn.
#[derive(Clone, Copy)]
enum Source {
    Connections,
    Reserve,
    Acceptor,
}

/// What woke the entity.
enum Woke<S, E> {
    Accepted(Result<S, E>),
    Socket(SlotRef, Io),
    Timer,
}

impl<
    'a,
    A: TcpAccept + 'a,
    const MCTS: usize,
    const MAX_MESSAGE: usize,
    const TESTERS: usize,
> fmt::Debug for Entity<'a, A, MCTS, MAX_MESSAGE, TESTERS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entity")
            .field("address", &self.address)
            .field("config", &self.config)
            .field(
                "open_connections",
                &self
                    .connections
                    .iter()
                    .filter(|slot| slot.open.is_some())
                    .count(),
            )
            .finish_non_exhaustive()
    }
}

impl<
    'a,
    A: TcpAccept + 'a,
    const MCTS: usize,
    const MAX_MESSAGE: usize,
    const TESTERS: usize,
> Entity<'a, A, MCTS, MAX_MESSAGE, TESTERS>
{
    /// An entity accepting connections from `acceptor`.
    ///
    /// Nothing is accepted until [`DiagnosticEntity::next_event`] is called.
    ///
    /// # Arguments
    ///
    /// * `acceptor` - the bound listening socket, usually on [`TCP_PORT`](crate::TCP_PORT).
    /// * `address` - the addresses the entity answers.
    /// * `config` - the testers that may activate routing.
    #[must_use]
    pub fn new(
        acceptor: &'a A,
        address: EntityAddress,
        config: EntityConfig<TESTERS>,
    ) -> Self {
        const {
            assert!(
                MCTS >= 1 && MCTS <= 255,
                "ISO 13400-2:2019 Table 11: MCTS is 1 to 255"
            );
            assert!(
                MAX_MESSAGE >= RESERVE_CAP,
                "MAX_MESSAGE must hold the reserve socket's frames"
            );
        };
        Self {
            acceptor,
            address,
            config,
            connections: core::array::from_fn(|_| Slot::new()),
            reserve: Slot::new(),
            arbitration: None,
            confirms: [None; CONFIRMS],
            turn: Turn::Connection(0),
        }
    }

    fn connection_id(index: usize) -> ConnectionId {
        ConnectionId::new(u8::try_from(index).unwrap_or(u8::MAX))
    }

    fn slot_ref(position: usize) -> SlotRef {
        if position < MCTS {
            SlotRef::Connection(position)
        } else {
            SlotRef::Reserve
        }
    }

    fn open_mut(&mut self, at: SlotRef) -> Option<&mut table::Open<A::Socket<'a>>> {
        match at {
            SlotRef::Connection(index) => self.connections.get_mut(index)?.open.as_mut(),
            SlotRef::Reserve => self.reserve.open.as_mut(),
        }
    }

    fn open(&self, at: SlotRef) -> Option<&table::Open<A::Socket<'a>>> {
        match at {
            SlotRef::Connection(index) => self.connections.get(index)?.open.as_ref(),
            SlotRef::Reserve => self.reserve.open.as_ref(),
        }
    }

    fn phase_of(&self, at: SlotRef) -> Option<Phase> {
        match at {
            SlotRef::Connection(index) => self.connections.get(index)?.phase(),
            SlotRef::Reserve => self.reserve.phase(),
        }
    }

    fn registered_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|slot| slot.registered_sa().is_some())
            .count()
    }

    fn holder_of(&self, sa: LogicalAddress) -> Option<usize> {
        self.connections
            .iter()
            .position(|slot| slot.registered_sa() == Some(sa))
    }

    /// A confirm that is due and owed to no earlier request to its target, then a close
    /// that is owed its report.
    fn owed_event(&mut self) -> Option<EntityEvent<'static>> {
        if let Some(event) = self.take_due_confirm() {
            return Some(event);
        }
        let (index, slot) = self
            .connections
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.closed_unreported)?;
        slot.closed_unreported = false;
        Some(EntityEvent::Closed {
            connection: Self::connection_id(index),
        })
    }

    fn take_due_confirm(&mut self) -> Option<EntityEvent<'static>> {
        let (at, result) = self.confirms.iter().enumerate().find_map(|(at, held)| {
            let confirm = held.as_ref()?;
            if self
                .confirms
                .iter()
                .take(at)
                .flatten()
                .any(|earlier| earlier.ta == confirm.ta)
            {
                return None;
            }
            let result = confirm.result.or_else(|| {
                self.connections
                    .get(confirm.connection)
                    .filter(|slot| slot.out.data.written_through(confirm.end))
                    .map(|_| DoIpResult::Ok)
            })?;
            Some((at, result))
        })?;
        let held = self.confirms.get_mut(at..)?;
        let confirm = held.first_mut()?.take()?;
        held.rotate_left(1);
        Some(EntityEvent::Confirm {
            sa: confirm.sa,
            ta: confirm.ta,
            ta_type: confirm.ta_type,
            result,
        })
    }

    fn queue(
        &mut self,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Refusal> {
        if pdu.is_empty() {
            return Err(Refusal::EmptyPdu);
        }
        if pdu.len() > Self::MAX_PDU {
            return Err(Refusal::PduTooLarge {
                len: pdu.len(),
                max: Self::MAX_PDU,
            });
        }
        let Some(free) = self.confirms.iter().position(Option::is_none) else {
            return Err(Refusal::NoRoom);
        };
        let mut confirm = PendingConfirm {
            connection: MCTS,
            end: 0,
            sa,
            ta,
            ta_type,
            result: Some(DoIpResult::UnknownSa),
        };
        if sa == self.address.physical {
            confirm.result = Some(DoIpResult::NoSocket);
            if let Some(index) = self.holder_of(ta)
                && let Some(Slot {
                    open: Some(open),
                    out,
                    ..
                }) = self.connections.get_mut(index)
                && matches!(open.phase, Phase::Registered { .. })
            {
                let message = Message::diagnostic_message(open.version, sa, ta, pdu);
                confirm = match out.push(&message) {
                    Ok(end) => PendingConfirm {
                        connection: index,
                        end,
                        result: None,
                        ..confirm
                    },
                    Err(Full) => PendingConfirm {
                        result: Some(DoIpResult::OutOfMemory),
                        ..confirm
                    },
                };
            }
        }
        if let Some(entry) = self.confirms.get_mut(free) {
            *entry = Some(confirm);
        }
        Ok(())
    }

    /// Takes the socket at `at` out of the table, settling the confirms of requests it
    /// never wrote.
    fn remove(&mut self, at: SlotRef) {
        match at {
            SlotRef::Connection(index) => {
                let Some(slot) = self.connections.get_mut(index) else {
                    return;
                };
                for confirm in self.confirms.iter_mut().flatten() {
                    if confirm.connection == index && confirm.result.is_none() {
                        confirm.result =
                            Some(if slot.out.data.written_through(confirm.end) {
                                DoIpResult::Ok
                            } else {
                                DoIpResult::NoSocket
                            });
                    }
                }
                drop(slot.remove());
            }
            SlotRef::Reserve => drop(self.reserve.remove()),
        }
    }

    fn finalize(&mut self, at: SlotRef, abort: bool, now: Instant) {
        match at {
            SlotRef::Connection(index) => {
                if let Some(slot) = self.connections.get_mut(index) {
                    slot.finalize(abort, false, now);
                }
            }
            SlotRef::Reserve => self.reserve.finalize(abort, false, now),
        }
    }

    /// Applies what a socket did. Data either way restarts a registered socket's
    /// general inactivity timer (REQ 3.DoIP-080).
    fn apply(&mut self, at: SlotRef, io: Io, now: Instant) {
        match io {
            Io::Wrote | Io::Read(_) => {
                if let Some(open) = self.open_mut(at)
                    && matches!(open.phase, Phase::Registered { .. })
                {
                    open.deadline = expiry(now, GENERAL_INACTIVITY);
                }
            }
            Io::Lost => self.finalize(at, true, now),
            Io::Closed => self.remove(at),
        }
    }

    /// Acts on every socket timer that has expired by `now`.
    fn expire(&mut self, now: Instant) {
        for position in 0..=MCTS {
            self.expire_one(Self::slot_ref(position), now);
        }
    }

    /// Acts on the socket timer at `at`, if it has expired by `now`.
    fn expire_one(&mut self, at: SlotRef, now: Instant) {
        let Some(open) = self.open_mut(at) else {
            return;
        };
        if open.deadline > now {
            return;
        }
        match open.phase {
            Phase::Initialized | Phase::Registered { .. } => {
                self.finalize(at, false, now);
            }
            Phase::Finalizing { abort: false, .. } => self.finalize(at, true, now),
            Phase::Finalizing { abort: true, .. } => self.remove(at),
        }
    }

    /// The earliest instant a socket timer, or the arbitration, needs the entity again.
    fn next_wake(&self) -> Option<Instant> {
        let sockets = self
            .connections
            .iter()
            .filter_map(|slot| slot.open.as_ref())
            .chain(self.reserve.open.as_ref())
            .map(|open| open.deadline);
        let arbitration = match self.arbitration {
            Some(Arbitration {
                stage:
                    Stage::AliveCheck { deadline, .. }
                    | Stage::Assign {
                        deadline: Some(deadline),
                    },
                ..
            }) => Some(deadline),
            _ => None,
        };
        sockets.chain(arbitration).min()
    }

    /// Puts a newly accepted socket in a free slot, or drops it if there is none.
    fn place(&mut self, socket: A::Socket<'a>, now: Instant) {
        let deadline = expiry(now, INITIAL_INACTIVITY);
        if let Some(slot) = self.connections.iter_mut().find(|slot| slot.is_free()) {
            slot.open(socket, deadline);
        } else if self.reserve.is_free() {
            self.reserve.open(socket, deadline);
        }
    }

    /// Queues the routing activation response `code` to `request` on `at`. Any code but
    /// success then closes the socket (Figure 22).
    fn respond(
        &mut self,
        at: SlotRef,
        request: Activation,
        code: RoutingActivationResponseCode,
        now: Instant,
    ) {
        let physical = self.address.physical;
        match at {
            SlotRef::Connection(index) => {
                if let Some(slot) = self.connections.get_mut(index) {
                    push_response(slot, request, physical, code);
                }
            }
            SlotRef::Reserve => push_response(&mut self.reserve, request, physical, code),
        }
        if code != RoutingActivationResponseCode::RoutingSuccessfullyActivated {
            self.finalize(at, false, now);
        }
    }

    /// Registers `request.sa` on the socket at `at`, moving a reserve socket into a
    /// connection slot first. False if no connection slot can take it yet.
    fn assign(&mut self, at: SlotRef, request: Activation, now: Instant) -> bool {
        let index = match at {
            SlotRef::Connection(index) => index,
            SlotRef::Reserve => {
                let reserve = &mut self.reserve;
                let Some(index) = self.connections.iter_mut().position(|slot| {
                    (slot.is_free() || slot.is_initialized())
                        && swap_with_reserve(slot, reserve)
                }) else {
                    return false;
                };
                index
            }
        };
        let Some(open) = self.open_mut(SlotRef::Connection(index)) else {
            return false;
        };
        open.phase = Phase::Registered {
            sa: request.sa,
            alive_check: AliveCheck::NotAsked,
        };
        open.deadline = expiry(now, GENERAL_INACTIVITY);
        self.respond(
            SlotRef::Connection(index),
            request,
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            now,
        );
        true
    }

    /// Figure 22's routing activation handler, then Figure 26's socket handler, for a
    /// request received on `at`.
    fn activate(&mut self, at: SlotRef, request: Activation, now: Instant) {
        use RoutingActivationResponseCode as Code;
        if !self.config.accepts(request.sa) {
            return self.respond(at, request, Code::DeniedUnknownSourceAddress, now);
        }
        if !matches!(
            request.activation_type,
            ActivationTypeCode::Default | ActivationTypeCode::RegulationRequired
        ) {
            return self.respond(
                at,
                request,
                Code::DeniedUnsupportedRoutingActivationType,
                now,
            );
        }
        if let Some(Phase::Registered { sa, .. }) = self.phase_of(at) {
            let code = if sa == request.sa {
                Code::RoutingSuccessfullyActivated
            } else {
                Code::DeniedSourceAddressAlreadyActivated
            };
            return self.respond(at, request, code, now);
        }
        let deadline = expiry(now, ALIVE_CHECK);
        let stage = if let Some(holder) = self.holder_of(request.sa) {
            if let Some(slot) = self.connections.get_mut(holder) {
                slot.set_alive_check(AliveCheck::Due);
            }
            Stage::AliveCheck {
                scope: AliveCheckScope::SocketOfSa,
                deadline,
            }
        } else if self.registered_count() < MCTS {
            if self.assign(at, request, now) {
                return;
            }
            Stage::Assign {
                deadline: Some(deadline),
            }
        } else {
            for slot in &mut self.connections {
                slot.set_alive_check(AliveCheck::Due);
            }
            Stage::AliveCheck {
                scope: AliveCheckScope::AllRegistered,
                deadline,
            }
        };
        self.arbitration = Some(Arbitration {
            on: at,
            request,
            stage,
        });
        self.send_alive_checks();
    }

    /// Queues an alive check request on every registered socket due one that has room
    /// (REQ 3.DoIP-134: registered sockets only).
    fn send_alive_checks(&mut self) {
        for slot in &mut self.connections {
            if slot.alive_check() == Some(AliveCheck::Due)
                && slot.out.control.has_room_for(ALIVE_CHECK_REQUEST)
                && let Some(open) = slot.open.as_ref()
            {
                slot.out
                    .control
                    .push(&Message::alive_check_request(open.version))
                    .ok();
                slot.set_alive_check(AliveCheck::Asked);
            }
        }
    }

    fn end_arbitration(&mut self) {
        self.arbitration = None;
        for slot in &mut self.connections {
            slot.set_alive_check(AliveCheck::NotAsked);
        }
    }

    fn is_silent(&self, index: usize) -> bool {
        self.connections
            .get(index)
            .and_then(Slot::alive_check)
            .is_some_and(|check| check != AliveCheck::Answered)
    }

    /// Moves the arbitration on as far as `now` allows (Figures 26 to 28). A failed
    /// alive check aborts its socket.
    fn arbitrate(&mut self, now: Instant) {
        use RoutingActivationResponseCode as Code;
        let Some(arbitration) = self.arbitration else {
            return;
        };
        if self.phase_of(arbitration.on) != Some(Phase::Initialized) {
            return self.end_arbitration();
        }
        self.send_alive_checks();
        let (on, request) = (arbitration.on, arbitration.request);
        let assign = Stage::Assign {
            deadline: Some(expiry(now, ALIVE_CHECK)),
        };
        let stage = match arbitration.stage {
            Stage::Assign { deadline } => Stage::Assign { deadline },
            Stage::AliveCheck {
                scope: AliveCheckScope::SocketOfSa,
                deadline,
            } => match self.holder_of(request.sa) {
                None => assign,
                Some(holder) if !self.is_silent(holder) => {
                    self.end_arbitration();
                    return self.respond(
                        on,
                        request,
                        Code::DeniedSourceAddressAlreadyRegistered,
                        now,
                    );
                }
                Some(holder) if deadline <= now => {
                    self.finalize(SlotRef::Connection(holder), true, now);
                    assign
                }
                Some(_) => return,
            },
            Stage::AliveCheck {
                scope: AliveCheckScope::AllRegistered,
                deadline,
            } => {
                let silent: usize =
                    (0..MCTS).filter(|index| self.is_silent(*index)).count();
                if self.registered_count() < MCTS {
                    assign
                } else if silent == 0 {
                    self.end_arbitration();
                    return self.respond(
                        on,
                        request,
                        Code::DeniedAllTcpSocketsRegisteredAndActive,
                        now,
                    );
                } else if deadline <= now {
                    for index in 0..MCTS {
                        if self.is_silent(index) {
                            self.finalize(SlotRef::Connection(index), true, now);
                        }
                    }
                    assign
                } else {
                    return;
                }
            }
        };
        let stage = match stage {
            Stage::Assign { .. } if self.assign(on, request, now) => {
                return self.end_arbitration();
            }
            Stage::Assign {
                deadline: Some(deadline),
            } if deadline <= now => {
                for index in 0..MCTS {
                    if self
                        .connections
                        .get(index)
                        .is_some_and(Slot::is_initialized)
                    {
                        self.finalize(SlotRef::Connection(index), true, now);
                    }
                }
                Stage::Assign { deadline: None }
            }
            stage => stage,
        };
        self.arbitration = Some(Arbitration {
            stage,
            ..arbitration
        });
    }

    /// Handles one buffered frame, from the first slot that has one.
    fn handle_one(&mut self, buf: &mut [u8], now: Instant) -> Step {
        let arbitrating = self.arbitration.map(|arbitration| arbitration.on);
        let limits = Limits {
            message: MAX_MESSAGE,
            unregistered: RESERVE_CAP,
        };
        for position in 0..=MCTS {
            let at = Self::slot_ref(position);
            if arbitrating == Some(at) {
                continue;
            }
            let handled = match at {
                SlotRef::Connection(index) => match self.connections.get_mut(index) {
                    Some(slot) => handler::handle(
                        slot,
                        self.address,
                        limits,
                        arbitrating.is_some(),
                        buf,
                        now,
                    ),
                    None => Handled::Waiting,
                },
                SlotRef::Reserve => handler::handle(
                    &mut self.reserve,
                    self.address,
                    limits,
                    arbitrating.is_some(),
                    buf,
                    now,
                ),
            };
            let connection = Self::connection_id(position);
            let step = match handled {
                Handled::Waiting => continue,
                Handled::Done => Step::Progress,
                Handled::Activation(request) => {
                    self.activate(at, request, now);
                    Step::Progress
                }
                Handled::Indication {
                    sa,
                    ta,
                    ta_type,
                    copied,
                    length,
                } => Step::Deliver(Delivery {
                    connection,
                    sa,
                    ta,
                    ta_type,
                    copied,
                    length,
                }),
            };
            return step;
        }
        Step::Idle
    }

    /// The deadline passed by `now` that judges the socket at `at`: its own timer, or the
    /// alive check it has not answered.
    fn judged_by(&self, at: SlotRef, now: Instant) -> Option<Instant> {
        let timer = self
            .open(at)
            .map(|open| open.deadline)
            .filter(|deadline| *deadline <= now);
        let alive_check = match (at, self.arbitration) {
            (
                SlotRef::Connection(index),
                Some(Arbitration {
                    stage: Stage::AliveCheck { deadline, .. },
                    ..
                }),
            ) if deadline <= now && self.is_silent(index) => Some(deadline),
            _ => None,
        };
        timer.or(alive_check)
    }

    /// Takes in what each socket a passed deadline judges has ready, so input that
    /// reached it in time is handled before the deadline is acted on: for each deadline,
    /// at most as much as the socket's receive buffer holds, so a peer that keeps sending
    /// holds it off no longer. Whether anything was taken in.
    async fn take_in(&mut self, now: Instant) -> bool {
        poll_fn(|cx| {
            let mut took = false;
            for position in 0..=MCTS {
                let at = Self::slot_ref(position);
                let Some(deadline) = self.judged_by(at, now) else {
                    continue;
                };
                let capacity = match at {
                    SlotRef::Connection(_) => MAX_MESSAGE,
                    SlotRef::Reserve => RESERVE_CAP,
                };
                let taken = match self.open(at).and_then(|open| open.taken_in) {
                    Some((judged, taken)) if judged == deadline => taken,
                    _ => 0,
                };
                if taken >= capacity {
                    continue;
                }
                let io = match at {
                    SlotRef::Connection(index) => self
                        .connections
                        .get_mut(index)
                        .and_then(|slot| poll_once(drive(slot), cx)),
                    SlotRef::Reserve => poll_once(drive(&mut self.reserve), cx),
                };
                if let Some(io) = io {
                    if let (Io::Read(read), Some(open)) = (io, self.open_mut(at)) {
                        open.taken_in = Some((deadline, taken.saturating_add(read)));
                    }
                    self.apply(at, io, now);
                    took = true;
                }
            }
            Poll::Ready(took)
        })
        .await
    }

    /// Waits for whichever comes first: a connection, a socket's next read or write,
    /// or `wake`. The source that wins is polled last next time, so none starves.
    async fn wait(&mut self, wake: Option<Instant>) -> Woke<A::Socket<'a>, A::Error> {
        use Source::{Acceptor, Connections, Reserve};
        let (first, order) = match self.turn {
            Turn::Connection(index) => (index, [Connections, Reserve, Acceptor]),
            Turn::Reserve => (0, [Reserve, Acceptor, Connections]),
            Turn::Acceptor => (0, [Acceptor, Connections, Reserve]),
        };
        let winner = {
            let acceptor = self.acceptor;
            let mut accept = pin!(acceptor.accept());
            let (before, from) = self
                .connections
                .split_at_mut_checked(first)
                .unwrap_or_default();
            let mut slots = from
                .iter_mut()
                .zip(first..)
                .chain(before.iter_mut().zip(0..));
            let mut connections =
                pin!(select_array(core::array::from_fn::<_, MCTS, _>(|_| {
                    let next = slots.next();
                    async move {
                        match next {
                            Some((slot, index)) => (index, drive(slot).await),
                            None => pending().await,
                        }
                    }
                })));
            let mut reserve = pin!(drive(&mut self.reserve));
            let mut timer = pin!(async {
                match wake {
                    Some(wake) => Timer::at(wake).await,
                    None => pending().await,
                }
            });
            poll_fn(|cx| {
                for source in order {
                    match source {
                        Connections => {
                            if let Poll::Ready(((index, io), _)) =
                                connections.as_mut().poll(cx)
                            {
                                return Poll::Ready(Woke::Socket(
                                    SlotRef::Connection(index),
                                    io,
                                ));
                            }
                        }
                        Reserve => {
                            if let Poll::Ready(io) = reserve.as_mut().poll(cx) {
                                return Poll::Ready(Woke::Socket(SlotRef::Reserve, io));
                            }
                        }
                        Acceptor => {
                            if let Poll::Ready(accepted) = accept.as_mut().poll(cx) {
                                return Poll::Ready(Woke::Accepted(
                                    accepted.map(|(_, socket)| socket),
                                ));
                            }
                        }
                    }
                }
                timer.as_mut().poll(cx).map(|()| Woke::Timer)
            })
            .await
        };
        self.turn = match &winner {
            Woke::Socket(SlotRef::Connection(index), _) => index
                .checked_add(1)
                .filter(|next| *index >= first && *next < MCTS)
                .map_or(Turn::Reserve, Turn::Connection),
            Woke::Socket(SlotRef::Reserve, _) => Turn::Acceptor,
            Woke::Accepted(_) => Turn::Connection(0),
            Woke::Timer => self.turn,
        };
        winner
    }
}

fn poll_once<F: Future>(future: F, cx: &mut Context<'_>) -> Option<F::Output> {
    match pin!(future).poll(cx) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

fn push_response<S, const CAP: usize>(
    slot: &mut Slot<S, CAP>,
    request: Activation,
    physical: LogicalAddress,
    code: RoutingActivationResponseCode,
) {
    let version = slot
        .open
        .as_ref()
        .map_or(ProtocolVersion::V2019, |open| open.version);
    let response = Message::routing_activation_response(
        version, request.sa, physical, code, [0; 4], None,
    );
    slot.out.control.push(&response).ok();
}

/// Exchanges `slot`'s socket and buffers with the reserve's, if both fit.
fn swap_with_reserve<S, const N: usize, const M: usize>(
    slot: &mut Slot<S, N>,
    reserve: &mut Slot<S, M>,
) -> bool {
    if slot.rx.swap(&mut reserve.rx).is_err() {
        return false;
    }
    if slot.out.swap(&mut reserve.out).is_err() {
        slot.rx.swap(&mut reserve.rx).ok();
        return false;
    }
    core::mem::swap(&mut slot.open, &mut reserve.open);
    true
}

impl<
    'a,
    A: TcpAccept + 'a,
    const MCTS: usize,
    const MAX_MESSAGE: usize,
    const TESTERS: usize,
> DiagnosticEntity for Entity<'a, A, MCTS, MAX_MESSAGE, TESTERS>
{
    type Error = Error<A::Error>;

    const CONNECTIONS: usize = MCTS + 1;

    /// `MAX_MESSAGE` less what a diagnostic message adds to its PDU,
    /// [`DIAGNOSTIC_MESSAGE_OVERHEAD`]. A response this long fits however many of the
    /// entity's own frames, its acknowledgements among them, are queued ahead of it:
    /// they queue apart.
    const MAX_PDU: usize = MAX_MESSAGE.saturating_sub(DIAGNOSTIC_MESSAGE_OVERHEAD);

    /// Queues `pdu` on the connection that registered `ta`. Its confirm is
    /// [`DoIpResult::Ok`] once [`DiagnosticEntity::next_event`] has written it.
    ///
    /// The entity's own logical address is its [`EntityAddress::physical`]; a request
    /// from any other `sa` sends nothing and is confirmed [`DoIpResult::UnknownSa`].
    ///
    /// Waits for nothing: the request is made by the future's first poll, which
    /// completes it, so a future dropped unpolled makes none. Where the connection's
    /// queue has no room for `pdu`, nothing is sent and the request is confirmed
    /// [`DoIpResult::OutOfMemory`]. Requests to one target are confirmed in the order
    /// they were made.
    ///
    /// # Errors
    ///
    /// None of these is followed by a confirm:
    /// - [`Refusal::PduTooLarge`] where `pdu` is longer than
    ///   [`DiagnosticEntity::MAX_PDU`].
    /// - [`Refusal::EmptyPdu`] where `pdu` is empty.
    /// - [`Refusal::NoRoom`] while too many earlier requests await their confirm.
    fn request(
        &mut self,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> impl Future<Output = Result<(), Refusal>> {
        poll_fn(move |_| Poll::Ready(self.queue(sa, ta, ta_type, pdu)))
    }

    fn now(&self) -> Timestamp {
        timestamp(Instant::now())
    }

    /// The next event on any connection.
    ///
    /// Answers routing activation, alive checks and every header the generic header
    /// handler refuses itself, acknowledges each diagnostic message it indicates, and
    /// closes sockets whose timers expire.
    ///
    /// # Errors
    ///
    /// [`Error::Accept`] where the acceptor fails.
    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<EntityEvent<'b>, Self::Error> {
        let until = deadline.map(|deadline| caller_deadline(deadline, Instant::now()));
        loop {
            let now = Instant::now();
            if let Some(event) = self.owed_event() {
                return Ok(event);
            }
            let step = match self.handle_one(buf, now) {
                Step::Idle if self.take_in(now).await => continue,
                Step::Idle => {
                    self.expire(now);
                    self.arbitrate(now);
                    if let Some(event) = self.owed_event() {
                        return Ok(event);
                    }
                    self.handle_one(buf, now)
                }
                step => step,
            };
            match step {
                Step::Deliver(delivery) => return Ok(delivery.into_event(buf)),
                Step::Progress => continue,
                Step::Idle => {}
            }
            if until.is_some_and(|until| until <= now) {
                return Ok(EntityEvent::Deadline);
            }
            let alarm = match (self.next_wake(), until) {
                (Some(timer), Some(until)) => Some(timer.min(until)),
                (timer, until) => timer.or(until),
            };
            let woke = self.wait(alarm).await;
            let now = Instant::now();
            match woke {
                Woke::Accepted(Ok(socket)) => self.place(socket, now),
                Woke::Accepted(Err(error)) => return Err(Error::Accept(error)),
                Woke::Socket(at, io) => self.apply(at, io, now),
                Woke::Timer => {}
            }
        }
    }

    /// Writes what is queued on `connection`, then closes it.
    ///
    /// The close is bounded: a socket that has not closed within [`ORDERLY_CLOSE_LIMIT`]
    /// is aborted, and one whose abort has not finished within
    /// [`TCP_TIMEOUT_ALIVE_CHECK`](crate::TCP_TIMEOUT_ALIVE_CHECK) is dropped. Only
    /// `connection`'s own timer is acted on meanwhile; every other socket's waits for
    /// the next [`DiagnosticEntity::next_event`].
    ///
    /// # Errors
    ///
    /// None: the close is bounded instead. A `connection` an event has named has left the
    /// table when it returns; one no event has named is left alone.
    async fn close(&mut self, connection: ConnectionId) -> Result<(), Self::Error> {
        let index = connection.index();
        let at = SlotRef::Connection(index);
        let Some(slot) = self.connections.get_mut(index) else {
            return Ok(());
        };
        slot.closed_unreported = false;
        if !slot.open.as_ref().is_some_and(|open| open.named) {
            return Ok(());
        }
        slot.finalize(false, true, Instant::now());
        loop {
            let Some(slot) = self.connections.get_mut(index) else {
                return Ok(());
            };
            let Some(deadline) = slot.open.as_ref().map(|open| open.deadline) else {
                return Ok(());
            };
            match select(drive(slot), Timer::at(deadline)).await {
                Either::First(io) => self.apply(at, io, Instant::now()),
                Either::Second(()) => self.expire_one(at, Instant::now()),
            }
        }
    }
}
