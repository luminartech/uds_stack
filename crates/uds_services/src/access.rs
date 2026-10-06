//! Where a sub-function or identifier is available, and which security levels unlock it.
//!
//! ISO 14229-1:2020 8.7.3.1 Figure 6 asks three questions of a sub-function before its
//! service runs — supported at all (0x12), supported in the active session (0x7E), and
//! unlocked (0x33) — and Figures 26 and 30 ask the same of an identifier. A service trait
//! answers all three at once with an [`Access`], or `None` where it does not support the
//! sub-function or identifier at all, so the answers cannot disagree and the pipeline asks
//! them in the figure's order.

use crate::SecurityLevel;
use uds_protocol::DiagnosticSessionType;

/// A set of diagnostic sessions, any of the `0x00`-`0x7F` values a
/// [`DiagnosticSessionType`] carries.
///
/// # Examples
///
/// ```
/// use uds_services::{DiagnosticSessionType as S, Sessions};
///
/// let non_default = Sessions::of(&[S::ProgrammingSession, S::ExtendedDiagnosticSession]);
/// assert!(non_default.contains(S::ExtendedDiagnosticSession));
/// assert!(!non_default.contains(S::DefaultSession));
/// assert!(Sessions::ALL.contains(S::DefaultSession));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sessions(u128);

impl Sessions {
    /// No session.
    pub const NONE: Self = Self(0);

    /// Every session, the default one included.
    pub const ALL: Self = Self(u128::MAX);

    /// The sessions in `sessions`.
    ///
    /// # Arguments
    ///
    /// * `sessions` - the members; see [`DiagnosticSessionType`].
    #[must_use]
    pub fn of(sessions: &[DiagnosticSessionType]) -> Self {
        sessions
            .iter()
            .fold(Self::NONE, |set, &session| set.with(session))
    }

    /// This set and `session`.
    ///
    /// # Arguments
    ///
    /// * `session` - the session added; see [`DiagnosticSessionType`].
    #[must_use]
    pub fn with(self, session: DiagnosticSessionType) -> Self {
        Self(self.0 | Self::bit(session))
    }

    /// Whether `session` is a member.
    ///
    /// # Arguments
    ///
    /// * `session` - the session asked about; see [`DiagnosticSessionType`].
    #[must_use]
    pub fn contains(self, session: DiagnosticSessionType) -> bool {
        self.0 & Self::bit(session) != 0
    }

    fn bit(session: DiagnosticSessionType) -> u128 {
        1 << (u8::from(session) & 0x7F)
    }
}

/// A set of security levels.
///
/// # Examples
///
/// ```
/// use uds_services::{Levels, SecurityLevel};
///
/// let oem = SecurityLevel::from_request_seed(0x01).expect("a requestSeed value");
/// let supplier = SecurityLevel::from_request_seed(0x11).expect("a requestSeed value");
/// let either = Levels::NONE.with(oem).with(supplier);
/// assert!(either.contains(supplier));
/// assert!(!Levels::NONE.contains(oem));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Levels(u64);

impl Levels {
    /// No level.
    pub const NONE: Self = Self(0);

    /// This set and `level`.
    ///
    /// # Arguments
    ///
    /// * `level` - the level added; see [`SecurityLevel`].
    #[must_use]
    pub const fn with(self, level: SecurityLevel) -> Self {
        Self(self.0 | Self::bit(level))
    }

    /// Whether `level` is a member.
    ///
    /// # Arguments
    ///
    /// * `level` - the level asked about; see [`SecurityLevel`].
    #[must_use]
    pub const fn contains(self, level: SecurityLevel) -> bool {
        self.0 & Self::bit(level) != 0
    }

    /// Whether the set has no member.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    const fn bit(level: SecurityLevel) -> u64 {
        1 << (level.request_seed() / 2)
    }
}

/// Where a supported sub-function or identifier is available, and which security levels
/// unlock it.
///
/// A service trait returns one for what it supports and `None` for what it does not
/// (0x12 for a sub-function, 0x31 for an identifier). The pipeline then settles a request
/// made outside [`Self::sessions`] (0x7E for a sub-function, 0x31 for an identifier), and
/// one made while none of [`Self::levels`] is unlocked (0x33).
///
/// # Examples
///
/// ```
/// use uds_services::{Access, DiagnosticSessionType as S, Levels, SecurityLevel, Sessions};
///
/// let oem = SecurityLevel::from_request_seed(0x01).expect("a requestSeed value");
/// let supplier = SecurityLevel::from_request_seed(0x11).expect("a requestSeed value");
///
/// // Anywhere, with nothing unlocked.
/// let open = Access::new(Sessions::ALL);
/// assert!(open.levels().is_empty());
///
/// // Only in the extended session, and only once either level is unlocked.
/// let secured = Access::new(Sessions::of(&[S::ExtendedDiagnosticSession]))
///     .unlocked_by(Levels::NONE.with(oem).with(supplier));
/// assert!(secured.levels().contains(supplier));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Access {
    sessions: Sessions,
    levels: Levels,
}

impl Access {
    /// Available in `sessions`, with no security level required.
    ///
    /// # Arguments
    ///
    /// * `sessions` - where it is available; see [`Sessions`].
    #[must_use]
    pub const fn new(sessions: Sessions) -> Self {
        Self {
            sessions,
            levels: Levels::NONE,
        }
    }

    /// The same, available only while one of `levels` is unlocked.
    ///
    /// # Arguments
    ///
    /// * `levels` - the levels any one of which unlocks it; see [`Levels`].
    ///   [`Levels::NONE`] requires none, as [`Self::new`] does.
    #[must_use]
    pub const fn unlocked_by(self, levels: Levels) -> Self {
        Self {
            sessions: self.sessions,
            levels,
        }
    }

    /// Where it is available.
    #[must_use]
    pub const fn sessions(self) -> Sessions {
        self.sessions
    }

    /// The levels any one of which unlocks it, empty where none is required.
    #[must_use]
    pub const fn levels(self) -> Levels {
        self.levels
    }

    /// Whether `unlocked`, the level the server holds unlocked, satisfies
    /// [`Self::levels`].
    pub(crate) fn admits(self, unlocked: Option<SecurityLevel>) -> bool {
        self.levels.is_empty() || unlocked.is_some_and(|level| self.levels.contains(level))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "the fixtures' values are valid by construction"
)]
mod tests {
    use super::{Access, Levels, Sessions};
    use crate::SecurityLevel;
    use uds_protocol::DiagnosticSessionType as S;

    fn level(request_seed: u8) -> SecurityLevel {
        SecurityLevel::from_request_seed(request_seed).expect("a requestSeed value")
    }

    /// Every `diagnosticSessionType` value has its own member, from 0x00 to 0x7F.
    #[test]
    fn sessions_hold_every_session_value() {
        let edges = [0x00_u8, 0x01, 0x40, 0x7E, 0x7F]
            .map(|value| S::try_from(value).expect("a diagnosticSessionType value"));
        for session in edges {
            assert!(
                Sessions::NONE.with(session).contains(session),
                "{session:?}"
            );
            assert!(Sessions::ALL.contains(session), "{session:?}");
            assert!(!Sessions::NONE.contains(session), "{session:?}");
        }
        let one = Sessions::of(&[S::ProgrammingSession]);
        assert!(!one.contains(S::ExtendedDiagnosticSession));
    }

    /// Every level `SecurityLevel` admits has its own member, from 0x01 to 0x7D.
    #[test]
    fn levels_hold_every_level() {
        for value in (0x01..0x7F).step_by(2) {
            let set = Levels::NONE.with(level(value));
            assert!(set.contains(level(value)), "{value:#04X}");
            for other in (0x01..0x7F).step_by(2).filter(|&other| other != value) {
                assert!(!set.contains(level(other)), "{value:#04X} vs {other:#04X}");
            }
        }
    }

    /// ISO 14229-1:2020 clause 10.4.1 allows one level unlocked at a time, and a resource
    /// more than one level opens is satisfied by any one of them; one that names none is
    /// satisfied with nothing unlocked.
    #[test]
    fn any_one_of_the_levels_admits() {
        let either = Access::new(Sessions::ALL)
            .unlocked_by(Levels::NONE.with(level(0x01)).with(level(0x11)));
        assert!(either.admits(Some(level(0x01))));
        assert!(either.admits(Some(level(0x11))));
        assert!(!either.admits(Some(level(0x03))));
        assert!(!either.admits(None));
        assert!(Access::new(Sessions::ALL).admits(None));
    }
}
