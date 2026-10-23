//! `RoutineControl` (0x31) over the layers' RID tables.

use core::fmt;
use core::marker::PhantomData;

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, ResponseSink, RoutineControl, RoutineControlSubFunction, RoutineIdentifier,
    Sink,
};

use super::{Policy, TableApp, settled};
use crate::layer::{Handler, UdsRidEntry, UdsRoutineFn};
use crate::op;

/// A routine identifier one of `P`'s layers serves.
pub struct Rid<P>(u16, PhantomData<fn() -> P>);

impl<P> Rid<P> {
    /// The identifier's value.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<P> Clone for Rid<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Rid<P> {}

impl<P> PartialEq for Rid<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P> Eq for Rid<P> {}

impl<P> fmt::Debug for Rid<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rid({:#06X})", self.0)
    }
}

impl<P: Policy> RoutineIdentifier for Rid<P> {
    /// A longer status is refused by [`TableApp::validate`].
    const MAX_STATUS_LEN: usize = P::MAX_STATUS_LEN;

    fn as_u16(self) -> u16 {
        self.0
    }

    fn from_u16(value: u16) -> Option<Self> {
        P::LAYERS.rid(value).map(|_| Self(value, PhantomData))
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> TableApp<P, RECORD, LEVELS> {
    /// Runs the step `pick` names of `routine`'s entry and answers the status record it
    /// copied, as many bytes as it copied.
    async fn step(
        &mut self,
        routine: Rid<P>,
        pick: fn(&UdsRidEntry) -> Option<UdsRoutineFn>,
        option: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let Some((step, len)) = P::LAYERS
            .rid(routine.0)
            .and_then(|e| Some((Handler(pick(e)?), usize::from(e.status_len))))
        else {
            return Err(Nrc::SubFunctionNotSupported);
        };
        let mut status = self.answer(len, P::MAX_STATUS_LEN)?;
        settled(op::settle(|op| status.lend(|| step.call(op, option))).await)?;
        let _ = out.write_all(status.filled());
        Ok(())
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> RoutineControl
    for TableApp<P, RECORD, LEVELS>
{
    type Rid = Rid<P>;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_OPTION_LEN: usize = P::MAX_OPTION_LEN;

    fn access(&self, routine: Rid<P>) -> Option<Access> {
        P::LAYERS.rid(routine.0).map(|e| e.access.access())
    }

    fn supports(&self, routine: Rid<P>, control: RoutineControlSubFunction) -> bool {
        P::LAYERS.rid(routine.0).is_some_and(|e| match control {
            RoutineControlSubFunction::StartRoutine => e.start.is_some(),
            RoutineControlSubFunction::StopRoutine => e.stop.is_some(),
            RoutineControlSubFunction::RequestRoutineResults => e.results.is_some(),
            RoutineControlSubFunction::IsoSaeReserved { .. } => false,
        })
    }

    async fn start(
        &mut self,
        routine: Rid<P>,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        self.step(routine, |e| e.start, record, out).await
    }

    async fn stop(
        &mut self,
        routine: Rid<P>,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        self.step(routine, |e| e.stop, record, out).await
    }

    async fn results(
        &mut self,
        routine: Rid<P>,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        self.step(routine, |e| e.results, record, out).await
    }
}
