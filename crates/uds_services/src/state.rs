//! Protocol state this crate keeps and the application cannot reach.
//!
//! ``UDSSVC_ARCH_0035`` — bookkeeping ISO 14229-1 needs across requests belongs here, out
//! of the application's reach: `State`'s fields are private to this crate and the
//! [`crate::Server`] that holds one keeps it in a private field. The assembly names the
//! type; the macro expands in the application's crate and could not keep a field private
//! there, which is why the type is declared here and only *named* by the macro.

use crate::SecurityLevel;
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
/// Transfer state joins as a field when its service lands.
#[derive(Debug)]
pub struct State {
    session: DiagnosticSessionType,
    /// ISO 14229-1:2020 clause 10.4.1: at most one level is unlocked at any instant.
    unlocked: Option<SecurityLevel>,
    /// Annex I's `xx`: the level whose seed was sent and whose key is awaited.
    seed_sent: Option<SecurityLevel>,
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

    pub(crate) fn take_seed(&mut self) -> Option<SecurityLevel> {
        self.seed_sent.take()
    }

    /// Annex I transitions 3 and 10: unlocking `level` locks whichever was unlocked.
    pub(crate) fn unlock(&mut self, level: SecurityLevel) {
        self.unlocked = Some(level);
    }

    /// Annex I transition 6: every level locked, no seed awaiting a key. Whether a level
    /// had been unlocked.
    pub(crate) fn lock(&mut self) -> bool {
        self.seed_sent = None;
        self.unlocked.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProtocolState, State};
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
}
