//! An [`Entity`](super::Entity)'s UDP: vehicle announcement and identification, entity
//! status and diagnostic power mode, on `UDP_DISCOVERY` (ISO 13400-2:2019 7.4 to 7.6).
//!
//! [`Entity::with_discovery`](super::Entity::with_discovery) gives an entity a bound UDP
//! socket and a [`VehicleIdentity`]; [`DiagnosticEntity::next_event`] then announces the
//! entity and answers on the socket, as it does on its `TCP_DATA` sockets.
//!
//! [`DiagnosticEntity::next_event`]: crate::service::DiagnosticEntity::next_event

mod datagram;

use core::fmt;
use core::future::{Future, pending};
use core::net::{Ipv4Addr, SocketAddr};

use edge_nal::{Readable, UdpReceive, UdpSend, UdpSplit};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant};

use crate::messages::{
    DiagnosticPowerModeCode, FurtherActionRequired, Header, ProtocolVersion,
    VinGidSyncStatus,
};
use crate::stream::after;
use crate::{EntityId, GroupId, Vin};
use datagram::{Answer, FRAME_CAP, Owed, When};
use sealed::DiscoveryIo;
pub(super) use sealed::Facts;

/// The longest request the entity takes: identification by VIN.
const LONGEST_REQUEST: usize = Header::SIZE + 17;
/// The receive buffer: the longest request, and as much again, so that a datagram too
/// long for its type is still read whole, and answered NACK `0x04`, on a socket that
/// discards a datagram that does not fit.
const RX_CAP: usize = 2 * LONGEST_REQUEST;
/// Answers a [`Discovery`] holds until it sends them.
const PENDING: usize = 4;
/// `A_DoIP_Announce_Wait`'s upper bound, in milliseconds (ISO 13400-2:2019 Table 12).
const ANNOUNCE_WAIT_MAX_MS: u32 = 500;
/// `A_DoIP_Announce_Interval` (Table 12).
const ANNOUNCE_INTERVAL: Duration = Duration::from_millis(500);
/// `A_DoIP_Announce_Num` (Table 12).
const ANNOUNCE_NUM: u8 = 3;
/// How long a socket that failed is left alone.
const REST: Duration = ANNOUNCE_INTERVAL;
/// Where an announcement goes: the IPv4 limited broadcast address (8.DoIP-125) and
/// `UDP_DISCOVERY` (4.DoIP-009).
const ANNOUNCE_TO: SocketAddr = SocketAddr::new(
    core::net::IpAddr::V4(Ipv4Addr::BROADCAST),
    crate::UDP_DISCOVERY_PORT,
);

/// What an entity announces itself as, and answers vehicle identification, power mode
/// and a directed request's EID with; and what it tells of a failing socket.
///
/// Read whenever a frame is built, so an implementation over the integrator's own
/// shared state, such as atomics or a `critical_section::Mutex`, changes what the next
/// frame carries. [`FixedIdentity`] is one whose values never change.
pub trait VehicleIdentity {
    /// The vehicle identification number, or `None` while none is programmed, which is
    /// sent as ISO 13400-2:2019 Table 1's all-`0x00` and matches no request.
    fn vin(&self) -> Option<Vin>;

    /// The entity ID, usually the MAC address of the interface the entity answers on.
    fn eid(&self) -> EntityId;

    /// The group ID, or `None` while none is set, which is sent as Table 1's all-`0x00`.
    fn gid(&self) -> Option<GroupId>;

    /// What a tester must do before diagnostics can proceed.
    fn further_action(&self) -> FurtherActionRequired;

    /// Whether the VIN and GID are synchronised, or `None` to omit the field.
    fn sync_status(&self) -> Option<VinGidSyncStatus>;

    /// The diagnostic power mode.
    fn power_mode(&self) -> DiagnosticPowerModeCode;

    /// Whether `eid`, from a request with an EID, names this entity: by default,
    /// whether it is [`VehicleIdentity::eid`]. An entity on several interfaces matches
    /// each of their addresses (ISO 13400-2:2019 REQ 8.DoIP-053).
    fn matches_eid(&self, eid: &[u8; 6]) -> bool {
        *eid == self.eid().to_bytes()
    }

    /// Told each time the UDP socket fails to receive or send, with its error: by
    /// default, nothing is done.
    ///
    /// The datagram is lost, and the entity leaves the socket alone for
    /// `A_DoIP_Announce_Interval` before using it again.
    /// [`DiagnosticEntity::next_event`] does not fail for it, so this is where a socket
    /// that keeps failing, leaving the entity unseen by testers, shows.
    ///
    /// [`DiagnosticEntity::next_event`]: crate::service::DiagnosticEntity::next_event
    fn discovery_failed<E: embedded_io_async::Error>(&self, _error: &E) {}
}

/// A [`VehicleIdentity`] whose values are fixed when it is built.
///
/// # Examples
///
/// ```
/// use simple_doip::entity::{FixedIdentity, VehicleIdentity};
/// use simple_doip::messages::DiagnosticPowerModeCode;
/// use simple_doip::{EntityId, Vin};
///
/// let eid = EntityId::new([0x02, 0, 0, 0, 0, 0x01])?;
/// let vin = Vin::new(*b"WVWZZZ1JZXW000001")?;
/// let identity = FixedIdentity::new(eid, DiagnosticPowerModeCode::Ready).with_vin(vin);
/// assert_eq!(identity.vin(), Some(vin));
/// assert_eq!(identity.gid(), None);
/// # Ok::<(), simple_doip::VinError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedIdentity {
    vin: Option<Vin>,
    eid: EntityId,
    gid: Option<GroupId>,
    further_action: FurtherActionRequired,
    sync_status: Option<VinGidSyncStatus>,
    power_mode: DiagnosticPowerModeCode,
}

impl FixedIdentity {
    /// An identity with no VIN or GID, no further action, and no sync status.
    ///
    /// # Arguments
    ///
    /// * `eid` - the entity ID; see [`VehicleIdentity::eid`].
    /// * `power_mode` - the diagnostic power mode.
    #[must_use]
    pub const fn new(eid: EntityId, power_mode: DiagnosticPowerModeCode) -> Self {
        Self {
            vin: None,
            eid,
            gid: None,
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            sync_status: None,
            power_mode,
        }
    }

    /// This identity with `vin` programmed.
    #[must_use]
    pub const fn with_vin(self, vin: Vin) -> Self {
        Self {
            vin: Some(vin),
            ..self
        }
    }

    /// This identity with `gid` set.
    #[must_use]
    pub const fn with_gid(self, gid: GroupId) -> Self {
        Self {
            gid: Some(gid),
            ..self
        }
    }

    /// This identity asking a tester for `further_action`.
    #[must_use]
    pub const fn with_further_action(self, further_action: FurtherActionRequired) -> Self {
        Self {
            further_action,
            ..self
        }
    }

    /// This identity reporting `sync_status`.
    #[must_use]
    pub const fn with_sync_status(self, sync_status: VinGidSyncStatus) -> Self {
        Self {
            sync_status: Some(sync_status),
            ..self
        }
    }
}

impl VehicleIdentity for FixedIdentity {
    fn vin(&self) -> Option<Vin> {
        self.vin
    }

    fn eid(&self) -> EntityId {
        self.eid
    }

    fn gid(&self) -> Option<GroupId> {
        self.gid
    }

    fn further_action(&self) -> FurtherActionRequired {
        self.further_action
    }

    fn sync_status(&self) -> Option<VinGidSyncStatus> {
        self.sync_status
    }

    fn power_mode(&self) -> DiagnosticPowerModeCode {
        self.power_mode
    }
}

/// What an [`Entity`](super::Entity) does on UDP: [`NoDiscovery`] or [`Discovery`].
///
/// Implemented by those two alone.
pub trait Discover: sealed::Discover {}

/// An entity with no UDP socket: it serves `TCP_DATA` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoDiscovery;

impl Discover for NoDiscovery {}

/// An entity's UDP socket, its [`VehicleIdentity`], and the answers it owes.
///
/// Built by [`Entity::with_discovery`](super::Entity::with_discovery).
pub struct Discovery<U: UdpSplit, I> {
    socket: U,
    identity: I,
    jitter: Jitter,
    rx: [u8; RX_CAP],
    burst: Burst,
    pending: [Option<Pending>; PENDING],
    rest_until: Option<Instant>,
}

impl<U: UdpSplit, I: fmt::Debug> fmt::Debug for Discovery<U, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Discovery")
            .field("identity", &self.identity)
            .field("burst", &self.burst)
            .field("pending", &self.pending)
            .field("rest_until", &self.rest_until)
            .finish_non_exhaustive()
    }
}

impl<U: UdpSplit, I: VehicleIdentity> Discovery<U, I> {
    pub(super) fn new(socket: U, identity: I, seed: u32) -> Self {
        let jitter = Jitter::new(seed, identity.eid().to_bytes());
        Self {
            socket,
            identity,
            jitter,
            rx: [0; RX_CAP],
            burst: Burst::Unstarted,
            pending: [None; PENDING],
            rest_until: None,
        }
    }

    /// Moves past `which`, sent or given up.
    fn done(&mut self, which: Due, now: Instant) {
        match which {
            Due::Announcement => {
                self.burst = match self.burst {
                    Burst::Next { left, .. } if left > 1 => Burst::Next {
                        at: after(now, ANNOUNCE_INTERVAL),
                        left: left.saturating_sub(1),
                    },
                    _ => Burst::Done,
                };
            }
            Due::Pending(index) => {
                if let Some(pending) = self.pending.get_mut(index) {
                    *pending = None;
                }
            }
        }
    }

    fn failed(&mut self, error: &U::Error, now: Instant) {
        self.identity.discovery_failed(error);
        self.rest_until = Some(after(now, REST));
    }

    /// The announcement or answer to send next, if one is due by `now`: the earliest.
    fn due(&self, now: Instant) -> Option<(Due, SocketAddr, Answer, ProtocolVersion)> {
        let announcement = match self.burst {
            Burst::Next { at, .. } if at <= now => Some((
                at,
                Due::Announcement,
                ANNOUNCE_TO,
                Answer::Announcement,
                ProtocolVersion::V2019,
            )),
            _ => None,
        };
        let answers =
            self.pending
                .iter()
                .copied()
                .zip(0..)
                .filter_map(|(pending, index)| {
                    pending.filter(|pending| pending.due <= now).map(|pending| {
                        (
                            pending.due,
                            Due::Pending(index),
                            pending.to,
                            pending.owed.answer,
                            pending.owed.version,
                        )
                    })
                });
        announcement
            .into_iter()
            .chain(answers)
            .min_by_key(|(at, ..)| *at)
            .map(|(_, due, to, answer, version)| (due, to, answer, version))
    }
}

/// The announcements still to send after a valid address is configured (8.DoIP-050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Burst {
    Unstarted,
    Next { at: Instant, left: u8 },
    Done,
}

/// An answer owed to `to`, due at `due`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    to: SocketAddr,
    owed: Owed,
    due: Instant,
}

/// Which frame a send was of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Due {
    Announcement,
    Pending(usize),
}

/// A xorshift over the integrator's seed and the entity's EID: each
/// `A_DoIP_Announce_Wait`, decorrelating entities powered up together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Jitter(u32);

impl Jitter {
    fn new(seed: u32, eid: [u8; 6]) -> Self {
        let [oui_0, oui_1, oui_2, nic_0, nic_1, nic_2] = eid;
        let folded = u32::from_be_bytes([oui_0 ^ nic_1, oui_1 ^ nic_2, oui_2, nic_0]);
        let mut state = seed ^ folded;
        state ^= state >> 16;
        state = state.wrapping_mul(0x7FEB_352D);
        state ^= state >> 15;
        state = state.wrapping_mul(0x846C_A68B);
        state ^= state >> 16;
        Self(if state == 0 { 0x9E37_79B9 } else { state })
    }

    fn announce_wait(&mut self) -> Duration {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        Duration::from_millis(u64::from(x % (ANNOUNCE_WAIT_MAX_MS + 1)))
    }
}

#[derive(Debug)]
enum Io<E> {
    Received { length: usize, from: SocketAddr },
    Sent(Due),
    SendFailed(Due, E),
    ReceiveFailed(E),
}

pub(super) mod sealed {
    use core::future::Future;

    use embassy_time::Instant;

    use crate::LogicalAddress;

    /// What a [`Discovery`](super::Discovery)'s socket did.
    #[derive(Debug)]
    pub struct DiscoveryIo<E>(pub(super) super::Io<E>);

    /// What the entity's connection table tells a discovery when it answers.
    #[derive(Debug, Clone, Copy)]
    pub struct Facts {
        pub address: LogicalAddress,
        pub mcts: u8,
        pub open: u8,
        pub max_data_size: u32,
    }

    pub trait Discover {
        type Io;

        /// Starts the announcement burst, the first time, and ends a rest that has
        /// passed.
        fn tick(&mut self, now: Instant);

        /// The next instant after `now` the socket needs the entity.
        fn next_wake(&self, now: Instant) -> Option<Instant>;

        /// Sends what is due by `now`, or receives, whichever completes first.
        fn drive(&mut self, now: Instant, facts: Facts) -> impl Future<Output = Self::Io>;

        fn apply(&mut self, io: Self::Io, now: Instant, facts: Facts);
    }
}

impl sealed::Discover for NoDiscovery {
    type Io = core::convert::Infallible;

    fn tick(&mut self, _: Instant) {}

    fn next_wake(&self, _: Instant) -> Option<Instant> {
        None
    }

    fn drive(&mut self, _: Instant, _: Facts) -> impl Future<Output = Self::Io> {
        pending()
    }

    fn apply(&mut self, io: Self::Io, _: Instant, _: Facts) {
        match io {}
    }
}

impl<U: UdpSplit, I: VehicleIdentity> Discover for Discovery<U, I> {}

impl<U: UdpSplit, I: VehicleIdentity> sealed::Discover for Discovery<U, I> {
    type Io = DiscoveryIo<U::Error>;

    fn tick(&mut self, now: Instant) {
        if self.burst == Burst::Unstarted {
            self.burst = Burst::Next {
                at: after(now, self.jitter.announce_wait()),
                left: ANNOUNCE_NUM,
            };
        }
        if self.rest_until.is_some_and(|until| until <= now) {
            self.rest_until = None;
        }
    }

    fn next_wake(&self, now: Instant) -> Option<Instant> {
        if let Some(until) = self.rest_until {
            return Some(until);
        }
        let announcement = match self.burst {
            Burst::Next { at, .. } => Some(at),
            Burst::Unstarted | Burst::Done => None,
        };
        self.pending
            .iter()
            .flatten()
            .map(|pending| pending.due)
            .chain(announcement)
            .filter(|at| *at > now)
            .min()
    }

    fn drive(&mut self, now: Instant, facts: Facts) -> impl Future<Output = Self::Io> {
        let due = self.due(now);
        let resting = self.rest_until.is_some();
        let Self {
            socket,
            identity,
            rx,
            ..
        } = self;
        async move {
            if resting {
                return pending().await;
            }
            let (mut receiver, mut sender) = socket.split();
            let receive = async {
                if let Err(error) = receiver.readable().await {
                    return Io::ReceiveFailed(error);
                }
                match receiver.receive(rx).await {
                    Ok((length, from)) => Io::Received { length, from },
                    Err(error) => Io::ReceiveFailed(error),
                }
            };
            let Some((which, to, answer, version)) = due else {
                return DiscoveryIo(receive.await);
            };
            let mut buf = [0; FRAME_CAP];
            let frame = datagram::frame(answer, version, facts, identity, &mut buf);
            let send = async {
                match sender.send(to, frame).await {
                    Ok(()) => Io::Sent(which),
                    Err(error) => Io::SendFailed(which, error),
                }
            };
            DiscoveryIo(match select(receive, send).await {
                Either::First(io) | Either::Second(io) => io,
            })
        }
    }

    fn apply(&mut self, io: Self::Io, now: Instant, facts: Facts) {
        match io.0 {
            Io::Received { length, from } => {
                let Some(owed) = datagram::handle(
                    self.rx.get(..length.min(RX_CAP)).unwrap_or_default(),
                    length,
                    from,
                    facts.max_data_size,
                    &self.identity,
                ) else {
                    return;
                };
                let due = match owed.when {
                    When::Now => now,
                    When::AfterAnnounceWait => after(now, self.jitter.announce_wait()),
                };
                if let Some(free) =
                    self.pending.iter_mut().find(|pending| pending.is_none())
                {
                    *free = Some(Pending {
                        to: from,
                        owed,
                        due,
                    });
                }
            }
            Io::Sent(which) => self.done(which, now),
            Io::SendFailed(which, error) => {
                self.done(which, now);
                self.failed(&error, now);
            }
            Io::ReceiveFailed(error) => self.failed(&error, now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 12: `A_DoIP_Announce_Wait` is random over 0 to 500 ms, from any seed,
    /// zero among them, which xorshift alone would never leave.
    #[test]
    fn the_announce_wait_covers_its_range_from_any_seed() {
        let most = Duration::from_millis(u64::from(ANNOUNCE_WAIT_MAX_MS));
        let eid = [0x02, 0, 0, 0, 0, 0x01];
        for seed in [0, 1, 0x1234_5678, u32::MAX] {
            let mut jitter = Jitter::new(seed, eid);
            let (mut shortest, mut longest) = (most, Duration::from_ticks(0));
            for _ in 0..10_000 {
                let wait = jitter.announce_wait();
                shortest = shortest.min(wait);
                longest = longest.max(wait);
            }
            assert!(
                shortest <= Duration::from_millis(5),
                "{seed:#x}: {shortest:?}"
            );
            assert!(
                longest >= Duration::from_millis(495),
                "{seed:#x}: {longest:?}"
            );
            assert!(longest <= most, "{seed:#x}: {longest:?}");
        }
    }

    /// Entities given the same seed, as two left at zero are, still wait apart: their
    /// EIDs differ, even in the last byte alone.
    #[test]
    fn entities_given_one_seed_wait_apart() {
        let waits = |eid| {
            let mut jitter = Jitter::new(0, eid);
            [(); 8].map(|()| jitter.announce_wait())
        };
        let first = waits([0x02, 0, 0, 0xAB, 0xCD, 0x01]);
        let second = waits([0x02, 0, 0, 0xAB, 0xCD, 0x02]);
        assert_ne!(first, second);
    }
}
