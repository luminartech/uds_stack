//! Protocol state this crate keeps and the application cannot reach.
//!
//! ``UDSSVC_ARCH_0035`` — bookkeeping ISO 14229-1 needs across requests belongs here, out
//! of the application's reach: `State`'s fields are private to this crate and the
//! [`crate::Server`] that holds one keeps it in a private field. The assembly names the
//! type; the macro expands in the application's crate and could not keep a field private
//! there, which is why the type is declared here and only *named* by the macro.

use crate::{SecurityLevel, Sessions};
use uds_protocol::DiagnosticSessionType;

/// The state an assembled server keeps between requests. ``UDSSVC_ARCH_0035``.
///
/// Sealed, so the only implementor is [`State`]; `INITIAL` rather than `EMPTY` because
/// the initial state is not zero — it is `defaultSession`.
///
/// The seal is this module's own, not [`crate::sealed`]'s. That one has to be nameable
/// from the application's crate, because `uds_server!` implements [`crate::ServiceSet`]
/// there; nothing outside this crate implements `ProtocolState` — the macro only names
/// [`State`] — so this seal can be one no other crate can reach, and a hand-written
/// state cannot slip past ``UDSSVC_ARCH_0035``.
///
/// Implementing it from outside this crate does not compile, even through the crate-wide
/// seal the macro uses:
///
/// ```compile_fail,E0277
/// use uds_services::ProtocolState;
///
/// #[derive(Debug)]
/// struct Mine;
/// impl uds_services::sealed::Sealed for Mine {}
/// impl ProtocolState for Mine {
///     const INITIAL: Self = Mine;
/// }
/// ```
pub trait ProtocolState: private::Sealed + core::fmt::Debug {
    /// The state at power-up: ISO 14229-1:2020 clause 10.2.1's default session.
    const INITIAL: Self;
}

/// The active diagnostic session and the security access state. ``UDSSVC_ARCH_0035``.
///
/// It also holds the download in progress, if any (``UDSSVC_ARCH_0036``).
#[derive(Debug)]
pub struct State {
    session: DiagnosticSessionType,
    /// ISO 14229-1:2020 clause 10.4.1: at most one level is unlocked at any instant.
    unlocked: Option<SecurityLevel>,
    /// Annex I's `xx`: the level whose seed was sent and whose key is awaited.
    seed_sent: Option<SecurityLevel>,
    /// The download in progress, if any.
    transfer: Option<Transfer>,
}

/// A download in progress (ISO 14229-1:2020 15.2 to 15.5, ``UDSSVC_ARCH_0036``).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transfer {
    /// Where its memory interval was admitted, within `DataTransfer::SESSIONS`: Figure 7
    /// key 3(iii) keeps it only in these.
    pub(crate) sessions: Sessions,
    /// The level unlocked at admission, where its `Access` required one: Figure 7 key
    /// 3(ii) ends it when that level is locked.
    pub(crate) gated_by: Option<SecurityLevel>,
    /// `memorySize`, where a `dataFormatIdentifier` of `0x00` makes it a count of the
    /// bytes transferred (Table 441).
    pub(crate) size: Option<u32>,
    /// Data bytes accepted, counted where `size` is known.
    pub(crate) received: u32,
    /// The `blockSequenceCounter` expected next (Table 451).
    pub(crate) next: u8,
    /// Whether a block has been accepted, so that a repeat is possible.
    pub(crate) accepted_any: bool,
}

impl Transfer {
    /// A transfer just opened: the counter expects `0x01` (Table 451).
    pub(crate) const fn new(
        sessions: Sessions,
        gated_by: Option<SecurityLevel>,
        size: Option<u32>,
    ) -> Self {
        Self {
            sessions,
            gated_by,
            size,
            received: 0,
            next: 0x01,
            accepted_any: false,
        }
    }

    /// Whether `counter` repeats the last accepted block (15.4.4, the 0x73 row).
    pub(crate) const fn is_repeat(&self, counter: u8) -> bool {
        self.accepted_any && counter == self.next.wrapping_sub(1)
    }

    /// Whether all of a known `memorySize` has been received (Table 454, the 0x24 row).
    pub(crate) fn all_received(&self) -> bool {
        self.size.is_some_and(|size| self.received >= size)
    }

    /// Whether less than a known `memorySize` has been received (Table 459, the 0x24 row).
    pub(crate) fn incomplete(&self) -> bool {
        self.size.is_some_and(|size| self.received < size)
    }

    /// Whether `len` more bytes would pass a known `memorySize` (Figure 33's 0x71 node).
    pub(crate) fn would_exceed(&self, len: usize) -> bool {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        self.size.is_some_and(|size| {
            u64::from(self.received).saturating_add(len) > u64::from(size)
        })
    }

    /// Accept a block of `len` data bytes: count them where `size` is known and advance
    /// the counter, `0xFF` rolling to `0x00`.
    pub(crate) fn accept(&mut self, len: usize) {
        if self.size.is_some() {
            let len = u32::try_from(len).unwrap_or(u32::MAX);
            self.received = self.received.saturating_add(len);
        }
        self.next = self.next.wrapping_add(1);
        self.accepted_any = true;
    }
}

/// The seal on [`ProtocolState`]: public in a private module, so no other crate can name
/// it and none can implement it.
mod private {
    /// Implemented by [`super::State`] alone.
    pub trait Sealed {}
}

impl private::Sealed for State {}

impl ProtocolState for State {
    const INITIAL: Self = Self {
        session: DiagnosticSessionType::DefaultSession,
        unlocked: None,
        seed_sent: None,
        transfer: None,
    };
}

impl State {
    /// The active session.
    #[must_use]
    pub(crate) const fn session(&self) -> DiagnosticSessionType {
        self.session
    }

    pub(crate) fn set_session(&mut self, session: DiagnosticSessionType) {
        self.session = session;
    }

    pub(crate) const fn unlocked(&self) -> Option<SecurityLevel> {
        self.unlocked
    }

    pub(crate) fn seed_sent(&mut self, level: SecurityLevel) {
        self.seed_sent = Some(level);
    }

    pub(crate) const fn awaited(&self) -> Option<SecurityLevel> {
        self.seed_sent
    }

    pub(crate) fn take_seed(&mut self) -> Option<SecurityLevel> {
        self.seed_sent.take()
    }

    pub(crate) const fn transfer(&self) -> Option<Transfer> {
        self.transfer
    }

    pub(crate) fn open_transfer(&mut self, transfer: Transfer) {
        self.transfer = Some(transfer);
    }

    pub(crate) fn accept_block(&mut self, len: usize) {
        if let Some(transfer) = self.transfer.as_mut() {
            transfer.accept(len);
        }
    }

    pub(crate) fn close_transfer(&mut self) {
        self.transfer = None;
    }

    /// Annex I transitions 3 and 10: unlocking `level` locks whichever was unlocked, which
    /// ends a transfer admitted under that one (Figure 7 key 3(ii)).
    pub(crate) fn unlock(&mut self, level: SecurityLevel) {
        if self
            .transfer
            .is_some_and(|t| t.gated_by.is_some_and(|g| g != level))
        {
            self.transfer = None;
        }
        self.unlocked = Some(level);
    }

    /// Annex I transition 6: every level locked, no seed awaiting a key, and a transfer
    /// that depended on an unlocked level ended (Figure 7 key 3(ii): "The locking of
    /// security access shall reset any active diagnostic functionality that was dependent
    /// on security access to be unlocked"). Whether a level had been unlocked.
    pub(crate) fn lock(&mut self) -> bool {
        self.seed_sent = None;
        if self.transfer.is_some_and(|t| t.gated_by.is_some()) {
            self.transfer = None;
        }
        self.unlocked.take().is_some()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "the fixtures' values are valid by construction"
)]
mod tests {
    use super::{ProtocolState, State, Transfer};
    use crate::{SecurityLevel, Sessions};
    use uds_protocol::DiagnosticSessionType;

    /// ISO 14229-1:2020 10.2.1 — a server powers up in the default session, and Annex I
    /// transition 1 in state A: every level locked, no seed sent.
    #[test]
    fn the_initial_state_is_the_default_session_and_locked() {
        let mut s = State::INITIAL;
        assert_eq!(s.session(), DiagnosticSessionType::DefaultSession);
        assert_eq!(s.unlocked(), None);
        assert_eq!(s.take_seed(), None);
    }

    /// A `static` server needs `INITIAL` to be a `const`.
    #[test]
    fn the_initial_state_is_usable_in_a_static() {
        static S: State = State::INITIAL;
        assert_eq!(S.session(), DiagnosticSessionType::DefaultSession);
    }

    fn level(seed: u8) -> SecurityLevel {
        SecurityLevel::from_request_seed(seed).expect("a requestSeed value")
    }

    fn open(gated_by: Option<SecurityLevel>) -> State {
        let mut s = State::INITIAL;
        s.open_transfer(Transfer::new(Sessions::ALL, gated_by, Some(16)));
        s
    }

    /// 15.4.2.2 Table 451 — the counter starts at 1 and rolls `FF` to `00`; a repeat is the
    /// last accepted counter, once one has been accepted.
    #[test]
    fn the_counter_starts_at_one_rolls_over_and_knows_a_repeat() {
        let mut t = Transfer::new(Sessions::ALL, None, None);
        assert_eq!(t.next, 0x01);
        assert!(!t.is_repeat(0x00), "nothing accepted yet");
        for _ in 0..0xFF {
            t.accept(1);
        }
        assert_eq!(t.next, 0x00);
        assert!(t.is_repeat(0xFF));
        t.accept(1);
        assert_eq!(t.next, 0x01);
        assert!(t.is_repeat(0x00), "the block after the roll");
    }

    /// Table 441 — `memorySize` is measured only where the format fixes its meaning.
    #[test]
    fn memory_size_is_counted_only_where_it_is_known() {
        let mut t = Transfer::new(Sessions::ALL, None, Some(16));
        assert!(t.incomplete());
        assert!(!t.would_exceed(16));
        assert!(t.would_exceed(17));
        t.accept(16);
        assert!(t.all_received());
        assert!(!t.incomplete());
        let unknown = Transfer::new(Sessions::ALL, None, None);
        assert!(!unknown.would_exceed(usize::MAX));
        assert!(!unknown.all_received());
        assert!(!unknown.incomplete());
    }

    /// Figure 7 key 3(ii) — locking ends a transfer that depended on an unlocked level and
    /// leaves one that did not.
    #[test]
    fn locking_discards_only_a_gated_transfer() {
        let mut gated = open(Some(level(0x03)));
        let _ = gated.lock();
        assert_eq!(gated.transfer(), None);
        let mut ungated = open(None);
        let _ = ungated.lock();
        assert!(ungated.transfer().is_some());
    }

    /// Annex I transitions 3 and 10, Figure 7 key 3(ii) — unlocking another level locks the
    /// one a transfer was admitted under, which ends it; unlocking the same one does not.
    #[test]
    fn unlocking_another_level_discards_the_transfer_it_locks() {
        let mut s = open(Some(level(0x03)));
        s.unlock(level(0x03));
        assert!(s.transfer().is_some());
        s.unlock(level(0x01));
        assert_eq!(s.transfer(), None);
    }
}
