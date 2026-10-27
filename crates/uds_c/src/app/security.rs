//! `SecurityAccess` (0x27) over the layers' security levels: the stack sequences seeds,
//! keys, attempts and delays, and the handlers make the seed and judge the key.

use core::num::NonZeroU8;

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Delay, KeyVerdict, ResponseSink, SecurityAccess, SecurityLevel, SecurityPolicy,
    Sessions, Sink,
};

use super::{Policy, TableApp, settled};
use crate::layer::{Handler, sessions};
use crate::op;

impl<P: Policy, const RECORD: usize, const LEVELS: usize> TableApp<P, RECORD, LEVELS> {
    /// The state of the delay of the level at `index`.
    fn delay_state(&mut self, index: usize) -> Delay {
        let Some(slot) = self.delay_until.get_mut(index) else {
            return Delay::Idle;
        };
        match *slot {
            None => Delay::Idle,
            Some(until) if embassy_time::Instant::now().as_millis() < until => {
                Delay::Running
            }
            Some(_) => {
                *slot = None;
                Delay::Expired
            }
        }
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> SecurityAccess
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_SEED_LEN: usize = P::MAX_SEED_LEN;
    const MAX_KEY_LEN: usize = P::MAX_KEY_LEN;
    const MAX_RECORD_LEN: usize = P::MAX_SECURITY_RECORD_LEN;

    /// `None`, so not supported, for a level no layer defines or one past `LEVELS`.
    fn sessions(&self, level: SecurityLevel) -> Option<Sessions> {
        Self::level(level).map(|(_, e)| sessions(e.sessions))
    }

    fn policy(&self, level: SecurityLevel) -> SecurityPolicy {
        match Self::level(level) {
            Some((_, e)) => SecurityPolicy::Counted {
                attempt_limit: NonZeroU8::new(e.attempt_limit).unwrap_or(NonZeroU8::MIN),
                delay_ms: (e.delay_ms != 0).then_some(e.delay_ms),
            },
            None => SecurityPolicy::RandomSeedOnly,
        }
    }

    fn preconditions_met(&self, _level: SecurityLevel) -> bool {
        true
    }

    fn load_attempts(&self, level: SecurityLevel) -> u8 {
        Self::level(level)
            .and_then(|(i, _)| self.attempts.get(i).copied())
            .unwrap_or(0)
    }

    fn store_attempts(&mut self, level: SecurityLevel, count: u8) {
        if let Some(slot) = Self::level(level).and_then(|(i, _)| self.attempts.get_mut(i)) {
            *slot = count;
        }
    }

    fn delay(&mut self, level: SecurityLevel) -> Delay {
        match Self::level(level) {
            Some((i, _)) => self.delay_state(i),
            None => Delay::Idle,
        }
    }

    fn start_delay(&mut self, level: SecurityLevel) {
        if let Some((i, e)) = Self::level(level) {
            let until = embassy_time::Instant::now()
                .as_millis()
                .saturating_add(u64::from(e.delay_ms));
            if let Some(slot) = self.delay_until.get_mut(i) {
                *slot = Some(until);
            }
        }
    }

    /// Answers the level's `seed_len` bytes, zero where the handler copied nothing. A seed
    /// left all zero, the value clause 10.4.1 reserves for an unlocked level, is
    /// `conditionsNotCorrect` (0x22).
    async fn seed(
        &mut self,
        level: SecurityLevel,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let Some((seed, len)) = Self::level(level)
            .and_then(|(_, e)| Some((Handler(e.seed?), usize::from(e.seed_len))))
        else {
            return Err(Nrc::SubFunctionNotSupported);
        };
        let mut answer = self.answer(len, P::MAX_SEED_LEN)?;
        settled(op::settle(|op| answer.lend(|| seed.call(op, record))).await)?;
        if answer.all().iter().all(|&byte| byte == 0) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        let _ = out.write_all(answer.all());
        Ok(())
    }

    async fn verify_key(
        &mut self,
        level: SecurityLevel,
        key: &[u8],
    ) -> Result<KeyVerdict, Nrc> {
        let Some(verify) = Self::level(level).and_then(|(_, e)| e.key).map(Handler) else {
            return Err(Nrc::SubFunctionNotSupported);
        };
        verdict(op::settle(|op| verify.call(op, key)).await)
    }
}

/// The key handler's result as the stack's verdict: the right key, a wrong one, or
/// another refusal.
fn verdict(nrc: u8) -> Result<KeyVerdict, Nrc> {
    match nrc {
        op::POSITIVE_RESPONSE => Ok(KeyVerdict::Valid),
        op::INVALID_KEY => Ok(KeyVerdict::Invalid),
        nrc => Err(Nrc::from(nrc)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_handler_result_is_the_verdict() {
        assert_eq!(verdict(0x00), Ok(KeyVerdict::Valid));
        assert_eq!(verdict(0x35), Ok(KeyVerdict::Invalid));
        assert_eq!(verdict(0x22), Err(Nrc::ConditionsNotCorrect));
    }
}
