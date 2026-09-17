//! Security access — ISO 14229-1:2020 clause 10.4 and Annex I.
//!
//! ``UDSSVC_ARCH_0037`` — the sequence is Annex I's Figure I.1, a four-state machine with
//! ten transitions, identical for every server, and it lives here. Its corrected topology
//! matters: transition 9 is `B → A` and transition 10 is `D → C`, so a failed `sendKey`
//! **discards the stored seed** and the client must request a new one.
//!
//! The application supplies the cryptography and the non-volatile storage, which are the
//! two things this crate cannot have. It never sees a state, never counts an attempt, and
//! never decides between 0x35 and 0x36.
//!
//! **There is no `Authentication` trait.** Service 0x29 has no `uds_protocol` message
//! type in either direction, so it decodes to `Request::Other` and settles 0x11 — a
//! server that wants it cannot implement it.

use crate::ResponseSink;
use uds_protocol::NegativeResponseCode;

/// How a level's attempts and delay are governed.
///
/// An enum because ISO 14229-1:2020 Table I.1 states a fallback — "if `Delay_Timer` and
/// `Att_Cnt` are not supported, a random seed shall always be used" — and an enum makes
/// declining both incompatible with a static seed rather than merely invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityPolicy {
    /// Neither counter nor delay: Table I.1's fallback, so the seed is always random.
    RandomSeedOnly,
    /// Counted attempts, with an optional delay and an optional static seed.
    Counted {
        /// Failed attempts permitted before the delay starts.
        attempt_limit: u8,
        /// How long the delay lasts, where one is kept.
        delay_ms: Option<u32>,
        /// Whether a repeated `requestSeed` returns the same seed.
        static_seed: bool,
    },
}

/// One security level: the `requestSeed`/`sendKey` pair, not a raw sub-function.
///
/// Clause 10.4.2 requires `sendKey` to be `requestSeed + 1`. Holding the pair makes that
/// structural, so Annex I's `0x24` answers only what it is meant to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecurityLevel(u8);

impl SecurityLevel {
    /// The level a `requestSeed` sub-function names, or `None` where it is not one.
    #[must_use]
    pub const fn from_request_seed(sub_function: u8) -> Option<Self> {
        if sub_function % 2 == 1 {
            Some(Self(sub_function))
        } else {
            None
        }
    }

    /// The `requestSeed` sub-function value.
    #[must_use]
    pub const fn request_seed(self) -> u8 {
        self.0
    }

    /// The `sendKey` sub-function value.
    #[must_use]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "from_request_seed admits only odd values, so +1 cannot overflow u8"
    )]
    pub const fn send_key(self) -> u8 {
        self.0 + 1
    }
}

/// Whether a key matched.
///
/// A verdict rather than a `Result`, because a wrong key is a normal outcome of the
/// exchange. Which negative response it produces — `invalidKey` (0x35) or
/// `exceedNumberOfAttempts` (0x36) — depends on the attempt count this crate maintains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyVerdict {
    /// Correct; Figure I.1 transition 3 or 10.
    Valid,
    /// Incorrect; Figure I.1 transition 9 or 10.
    Invalid,
}

/// `SecurityAccess` (0x27).
///
/// ``UDSSVC_ARCH_0034`` puts the attempt counter in the stack rather than the
/// application: clause 10.4 makes it server-global, so a per-application implementation
/// would be the same code written many times, wrong differently each time.
pub trait SecurityAccess {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The longest seed this server issues.
    const MAX_SEED_LEN: usize;

    /// The longest key this server accepts.
    const MAX_KEY_LEN: usize;

    /// How `level`'s attempts and delay are governed.
    fn policy(&self, level: SecurityLevel) -> SecurityPolicy;

    /// The attempt count for `level`, as last stored.
    ///
    /// The application never computes one — it persists the number this crate hands it,
    /// because Annex I requires the count to survive a power cycle and only the
    /// application has non-volatile storage.
    fn load_attempts(&self, level: SecurityLevel) -> u8;

    /// Store the count this crate computed.
    fn store_attempts(&mut self, level: SecurityLevel, count: u8);

    /// Whether a delay is currently running for `level`.
    ///
    /// Figure I.1 transition 4 — a request arriving while the delay runs is answered
    /// `requiredTimeDelayNotExpired` (0x37) without consulting the key.
    fn delay_running(&self, level: SecurityLevel) -> bool;

    /// Begin the delay this crate decided is owed.
    fn start_delay(&mut self, level: SecurityLevel);

    /// Write a seed for `level` into `out`.
    ///
    /// Never called for an already-unlocked level: Figure I.1 transition 7 fixes that
    /// answer as an all-zero seed, and this crate supplies it, because it knows what is
    /// unlocked and the application does not.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where a seed cannot be produced.
    fn seed(
        &mut self,
        level: SecurityLevel,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// Check `key` against the seed last issued for `level`.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where the check could not be performed at all. A key
    /// that is simply wrong is [`KeyVerdict::Invalid`], not an error.
    fn verify_key(
        &mut self,
        level: SecurityLevel,
        key: &[u8],
    ) -> impl core::future::Future<Output = Result<KeyVerdict, NegativeResponseCode>>;
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::doc_markdown,
    reason = "a test fixture may panic on its own setup, and the doc comments here quote \
              ISO 14229-1:2020 Table I.1's parameter names verbatim rather than restyling \
              them as code"
)]
mod tests {
    use super::{KeyVerdict, SecurityLevel, SecurityPolicy};

    /// Clause 10.4.2 pairs `requestSeed` (odd) with `sendKey` (the next even value).
    /// `SecurityLevel` holds the pair, so `yy == xx + 1` is structural and Annex I's
    /// 0x24 answers only what it is meant to: a `sendKey` for a level whose seed was
    /// never requested.
    #[test]
    fn a_level_is_the_pair_not_a_raw_sub_function() {
        let l = SecurityLevel::from_request_seed(0x01).expect("0x01 is a requestSeed");
        assert_eq!((l.request_seed(), l.send_key()), (0x01, 0x02));
        assert_eq!(SecurityLevel::from_request_seed(0x02), None);
    }

    /// Table I.1's fallback: "if Delay_Timer and Att_Cnt are not supported, a random
    /// seed shall always be used". As an enum, a deployment declining both cannot also
    /// set Static_Seed — the combination is unrepresentable rather than rejected.
    #[test]
    fn declining_the_counters_makes_a_static_seed_unrepresentable() {
        let fallback = SecurityPolicy::RandomSeedOnly;
        let counted = SecurityPolicy::Counted {
            attempt_limit: 3,
            delay_ms: Some(10_000),
            static_seed: true,
        };
        assert_ne!(fallback, counted);
    }

    /// A wrong key is a verdict, not an error: which negative response it produces
    /// (0x35 or 0x36) depends on the attempt count this crate maintains, so the
    /// application cannot choose it.
    #[test]
    fn a_wrong_key_is_a_verdict() {
        assert_ne!(KeyVerdict::Valid, KeyVerdict::Invalid);
    }
}
