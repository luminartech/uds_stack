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

use crate::{ResponseSink, Sessions};
use uds_protocol::NegativeResponseCode;

/// How a level's attempts and delay are governed.
///
/// ISO 14229-1:2020 Table I.1 makes the attempt counter and the delay optional, and
/// states a fallback for declining both — "a random seed shall always be used" — which
/// [`SecurityAccess::seed`] must honour under [`SecurityPolicy::RandomSeedOnly`]. Whether
/// a seed is static is otherwise the seed's own property, and [`SecurityAccess::seed`]'s
/// to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityPolicy {
    /// Neither counter nor delay: Table I.1's fallback, so the seed is always random.
    RandomSeedOnly,
    /// Counted attempts, with an optional delay.
    Counted {
        /// Failed attempts permitted before the delay starts.
        attempt_limit: u8,
        /// How long the delay lasts, where one is kept.
        delay_ms: Option<u32>,
    },
}

/// The state of a level's delay timer, as [`SecurityAccess::delay`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Delay {
    /// The delay is running: a `requestSeed` is `requiredTimeDelayNotExpired` (0x37).
    Running,
    /// The delay has run out since [`SecurityAccess::delay`] last reported it. Reported
    /// once per expiry: the next report is [`Delay::Idle`].
    Expired,
    /// No delay is running, and none has run out unreported.
    #[default]
    Idle,
}

/// One security level: the `requestSeed`/`sendKey` pair, not a raw sub-function.
///
/// Clause 10.4.2 requires `sendKey` to be `requestSeed + 1`. Holding the pair makes that
/// structural, so Annex I's `0x24` answers only what it is meant to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecurityLevel(u8);

impl SecurityLevel {
    /// The level a `requestSeed` sub-function names, or `None` where it is not one.
    ///
    /// A `requestSeed` sub-function is odd by construction, and must leave room for its
    /// `sendKey` partner: a sub-function byte is seven bits, because bit 7 is
    /// `suppressPosRspMsgIndication`, so both halves of the pair have to fall in
    /// `0x00`–`0x7F`. `0x7F` itself is rejected because its partner would be `0x80`,
    /// which is not a sub-function value at all.
    ///
    /// # Examples
    ///
    /// ```
    /// use uds_services::SecurityLevel;
    ///
    /// let level = SecurityLevel::from_request_seed(0x01)
    ///     .expect("0x01 is a requestSeed sub-function");
    /// assert_eq!(level.request_seed(), 0x01);
    /// assert_eq!(level.send_key(), 0x02);
    ///
    /// // An even sub-function is a `sendKey`; it names no level of its own.
    /// assert!(SecurityLevel::from_request_seed(0x02).is_none());
    /// // 0x7F is rejected because its partner would be 0x80, which is not a
    /// // sub-function value at all.
    /// assert!(SecurityLevel::from_request_seed(0x7F).is_none());
    /// ```
    #[must_use]
    pub const fn from_request_seed(sub_function: u8) -> Option<Self> {
        if sub_function % 2 == 1 && sub_function < 0x7F {
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
    pub const fn send_key(self) -> u8 {
        self.0.saturating_add(1)
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
    ///
    /// Also the width of the all-zero seed this crate answers a `requestSeed` with for the
    /// level already unlocked (clause 10.4.1, Annex I transition 7): every level's zero
    /// seed is this long, whatever the length of the seeds [`Self::seed`] writes for it.
    const MAX_SEED_LEN: usize;

    /// The longest key this server accepts.
    const MAX_KEY_LEN: usize;

    /// The longest `securityAccessDataRecord` this server accepts with a `requestSeed`
    /// (clause 10.4.2.3); zero where it takes none.
    ///
    /// A longer record is `incorrectMessageLengthOrInvalidFormat` (0x13) without
    /// [`Self::seed`] being asked. Folded into the request buffer by
    /// [`crate::uds_server`], as [`Self::MAX_KEY_LEN`] is.
    const MAX_RECORD_LEN: usize;

    /// The sessions `level` is available in, or `None` where this server does not
    /// support it.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, ``UDSSVC_ARCH_0007`` — asked for the
    /// `requestSeed` and the `sendKey` of the level alike: `None` settles the request
    /// `subFunctionNotSupported` (0x12), and an active session outside the set
    /// `subFunctionNotSupportedInActiveSession` (0x7E). A set rather than an
    /// [`Access`](crate::Access) because this is the service that unlocks: no level is
    /// required to ask for one. The default session never reaches here: Table 23 refuses
    /// the service there with 0x7F.
    ///
    /// # Arguments
    ///
    /// * `level` - the level the sub-function names; see [`SecurityLevel`].
    fn sessions(&self, level: SecurityLevel) -> Option<Sessions>;

    /// How `level`'s attempts and delay are governed.
    fn policy(&self, level: SecurityLevel) -> SecurityPolicy;

    /// Whether Annex I's optional pre-conditions for a `requestSeed` of `level` are met.
    ///
    /// ISO 14229-1:2020 Annex I, Table I.2 transition 4: a `false` is
    /// `conditionsNotCorrect` (0x22), decided before the delay's
    /// `requiredTimeDelayNotExpired` (0x37) and before an unlocked level's zero seed.
    /// Return `true` where the server keeps none.
    ///
    /// # Arguments
    ///
    /// * `level` - the level the `requestSeed` names; see [`SecurityLevel`].
    fn preconditions_met(&self, level: SecurityLevel) -> bool;

    /// The attempt count for `level`, as last stored.
    ///
    /// The application never computes one — it persists the number this crate hands it,
    /// because Annex I requires the count to survive a power cycle and only the
    /// application has non-volatile storage.
    fn load_attempts(&self, level: SecurityLevel) -> u8;

    /// Store the count this crate computed.
    fn store_attempts(&mut self, level: SecurityLevel, count: u8);

    /// The state of `level`'s delay timer, which the application runs.
    ///
    /// Asked only for a level whose [`Self::policy`] keeps a delay: at start-up, and on
    /// each `requestSeed` once its pre-conditions are met. [`Delay::Running`] answers the
    /// request `requiredTimeDelayNotExpired` (0x37) (Table I.2 transition 4).
    /// [`Delay::Expired`] has this crate reset the attempt count, as the timer's
    /// expiry does in Table I.2. [`Delay::Idle`] with the stored count at the limit — the
    /// state after a restart, which a RAM timer does not survive — has this crate call
    /// [`Self::start_delay`] (transition 1, "Start `Delay_Timer` … if required on start
    /// up"). So a lockout outlives a power cycle.
    ///
    /// # Arguments
    ///
    /// * `level` - the level asked about; see [`SecurityLevel`].
    fn delay(&mut self, level: SecurityLevel) -> Delay;

    /// Begin the delay this crate decided is owed.
    fn start_delay(&mut self, level: SecurityLevel);

    /// Write a seed for `level` into `out`.
    ///
    /// Never called for an already-unlocked level: Figure I.1 transition 7 fixes that
    /// answer as an all-zero seed, and this crate supplies it, because it knows what is
    /// unlocked and the application does not. So the seed written here must not be all
    /// zero: clause 10.4.1 forbids that for a locked level, and a debug build panics on
    /// one.
    ///
    /// Whether a repeated `requestSeed` gets the same seed is this method's to decide
    /// (Table I.1 `Static_Seed`, vehicle-manufacturer specific). A static seed is returned
    /// again for `level` until [`Self::verify_key`] reports [`KeyVerdict::Valid`] for it,
    /// after which Table I.2 transitions 3 and 10 have it cleared, so the next is new.
    /// Under [`SecurityPolicy::RandomSeedOnly`] every seed is fresh (Table I.1).
    ///
    /// # Arguments
    ///
    /// * `level` - the level the `requestSeed` names; see [`SecurityLevel`].
    /// * `record` - the request's `securityAccessDataRecord`, empty where it carried
    ///   none, and never longer than [`Self::MAX_RECORD_LEN`].
    /// * `out` - where the seed is written, after the `67` and the echoed sub-function
    ///   the pipeline wrote.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where a seed cannot be produced, such as
    /// `requestOutOfRange` (0x31) for a `record` holding invalid data (clause 10.4.4).
    /// Unmet pre-conditions are [`Self::preconditions_met`]'s, not this method's.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn seed(
        &mut self,
        level: SecurityLevel,
        record: &[u8],
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
    use super::{KeyVerdict, SecurityLevel};

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

    /// The pair must fit the seven-bit sub-function range: 0xFF is odd, but its partner
    /// would overflow, and 0x7F's partner would be 0x80, which is not a sub-function.
    #[test]
    fn a_level_whose_partner_would_not_fit_is_rejected() {
        assert_eq!(SecurityLevel::from_request_seed(0xFF), None);
        assert_eq!(SecurityLevel::from_request_seed(0x7F), None);
        assert_eq!(
            SecurityLevel::from_request_seed(0x7D).map(SecurityLevel::send_key),
            Some(0x7E)
        );
    }

    /// A wrong key is a verdict, not an error: which negative response it produces
    /// (0x35 or 0x36) depends on the attempt count this crate maintains, so the
    /// application cannot choose it.
    #[test]
    fn a_wrong_key_is_a_verdict() {
        assert_ne!(KeyVerdict::Valid, KeyVerdict::Invalid);
    }
}
