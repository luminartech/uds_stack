//! The crate's outward interface.
//!
//! [`DoIpTransport`] carries the methods `uds_services::UdsTransport`
//! requires. The `impl` block arrives once that crate publishes the trait;
//! until then these are inherent methods with the agreed signatures, so the
//! implementation is exercised rather than blocked.
//!
//! Nothing here is shaped by `uds_services`: the trait's own test is that a
//! CAN binding implements the same methods, so a `DoIP`-shaped seam would be
//! the wrong seam.

use crate::error::Error;
use crate::mapping::target_of;
use crate::profile::{Reloads, Timing};
use uds_session::{Ai, SResult};

/// The driver's view of what arrived, or that its deadline passed first.
///
/// Mirrors `uds_services::TransportEvent` exactly; `mapping::DoIpEvent`'s
/// other two cases — a periodic response and a connection close — are handled
/// inside this module and do not cross the seam.
#[derive(Debug)]
pub enum TransportEvent<'a> {
    /// A complete inbound message.
    DataInd {
        /// Addressing, with the responder's `S_AI[SA]` — the only way to tell
        /// functional responses apart.
        ai: Ai,
        /// The UDS payload.
        data: &'a [u8],
    },
    /// The outcome of a requested transmission.
    ///
    /// Raised from the diagnostic message **acknowledgement**, never from the
    /// socket write returning, because the acknowledgement is what starts
    /// `tP_Client` (ISO 14229-2:2021 REQ 5.9).
    DataConf {
        /// The addressing of the transmission being confirmed.
        ai: Ai,
        /// `SResult::Ok` for a `0x8002`; a `0x8003` is one
        /// `SResult::Transport` value.
        result: SResult,
    },
    /// The deadline the driver supplied passed before anything arrived.
    Deadline,
}

/// ISO 14229-5 over `DoIP`.
///
/// `S` is the socket, so no runtime is named: this builds for a bare-metal
/// target as readily as for tokio, and an adapter for either is additive.
pub struct DoIpTransport<S> {
    #[expect(dead_code, reason = "read once t_data_req and next_event leave todo!()")]
    socket: S,
    timing: Timing,
    inbound_max: Option<usize>,
    outbound_max: Option<usize>,
}

// Written by hand rather than derived. `#[derive(Debug)]` here would generate
// a conditional `impl<S: Debug> Debug for DoIpTransport<S>`; on this toolchain
// `missing_debug_implementations` happens to accept that as covering the
// type, but the conditional impl is still the wrong API: it leaves
// `DoIpTransport<S>` with no `Debug` at all for any socket that does not
// itself implement `Debug`, which is the common case rather than the
// exception. An unconditional impl that treats the socket as opaque covers
// every `S`.
impl<S> core::fmt::Debug for DoIpTransport<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpTransport")
            .field("socket", &"..")
            .field("timing", &self.timing)
            .field("inbound_max", &self.inbound_max)
            .field("outbound_max", &self.outbound_max)
            .finish()
    }
}

impl<S> DoIpTransport<S> {
    /// A transport over `socket` with `timing`, advertising no size bound in
    /// either direction until one is learned.
    pub const fn new(socket: S, timing: Timing) -> Self {
        Self {
            socket,
            timing,
            inbound_max: None,
            outbound_max: None,
        }
    }

    /// Record this entity's own *Max. data size*, learned from its
    /// configuration or from its entity status response.
    pub fn set_inbound_max(&mut self, max: Option<usize>) {
        self.inbound_max = max;
    }

    /// Record the peer's advertised *Max. data size*, learned from the peer's
    /// entity status response.
    ///
    /// A server typically has not requested one, which is why this stays
    /// `None` and `responseTooLong` is then unreachable rather than fabricated.
    pub fn set_outbound_max(&mut self, max: Option<usize>) {
        self.outbound_max = max;
    }

    /// The largest `A_PDU` this entity will accept, where it advertises one.
    ///
    /// ISO 13400-2:2019 Table 11 — support for *Max. data size* is
    /// **optional**, so `None` is conformant.
    #[must_use]
    pub const fn inbound_max(&self) -> Option<usize> {
        self.inbound_max
    }

    /// The largest `A_PDU` the peer will accept, where it has advertised one.
    ///
    /// This is what bounds a *response*: MDS is defined as the maximum size of
    /// one logical **request** the entity can process, so a server asking what
    /// it may send is asking about the client.
    #[must_use]
    pub const fn outbound_max(&self) -> Option<usize> {
        self.outbound_max
    }

    /// The `tP_Client` reload pair this transport dictates.
    #[must_use]
    pub const fn channel_timing(&self) -> Reloads {
        self.timing.reloads
    }

    /// Monotonic milliseconds, 32-bit and wrapping.
    #[must_use]
    pub fn now_ms(&self) -> u32 {
        todo!("clock source is the socket adapter's; see design doc decision 1")
    }

    /// `T_Data.req` — map a `T_PDU` onto a `DoIP` diagnostic message and send it.
    ///
    /// # Errors
    ///
    /// [`Error`] if the addressing cannot be carried — the two remote message
    /// types have no `DoIP` representation — or if the socket fails.
    #[expect(
        unused_variables,
        reason = "data is unused until t_data_req's body replaces the todo!() below"
    )]
    pub async fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> Result<(), Error> {
        let _target = target_of(ai)?;
        todo!("REQ 4.3 Table 4 — send as a DoIP diagnostic message")
    }

    /// The next inbound event, or [`TransportEvent::Deadline`] when
    /// `deadline_ms` passes first.
    ///
    /// The deadline is the session layer's `next_deadline_ms`, so this
    /// transport never invents one.
    ///
    /// # Errors
    ///
    /// [`Error`] if the socket fails.
    #[expect(
        unused_variables,
        reason = "deadline_ms is unused until next_event's body replaces the todo!() below"
    )]
    pub async fn next_event(
        &mut self,
        deadline_ms: Option<u32>,
    ) -> Result<TransportEvent<'_>, Error> {
        todo!("read a DoIP message, mapping::classify it, translate the two seam cases")
    }
}

#[cfg(test)]
mod tests {
    /// ISO 13400-2:2019 Table 11 lists *Max. data size* as the fourth item of
    /// the entity status response and marks its support **optional**, so a
    /// conformant `DoIP` entity need not advertise one and `None` is a correct
    /// answer rather than a defect.
    ///
    /// Design doc §1 decision 3: `uds_services` bounds the response sink only
    /// where a bound is known, and never fabricates one.
    #[test]
    fn an_unadvertised_max_data_size_is_none_not_a_guess() {
        let t = super::DoIpTransport::new((), crate::profile::Timing::default());
        assert_eq!(t.inbound_max(), None);
        assert_eq!(t.outbound_max(), None);
    }

    /// MDS is "the maximum size of one logical **request** that this `DoIP`
    /// entity can process", so the two directions are different questions and
    /// answering one does not answer the other.
    #[test]
    fn the_two_directions_are_independent() {
        let mut t = super::DoIpTransport::new((), crate::profile::Timing::default());
        t.set_inbound_max(Some(4096));
        assert_eq!(t.inbound_max(), Some(4096));
        assert_eq!(
            t.outbound_max(),
            None,
            "this entity's own MDS says nothing about what the peer will accept"
        );
    }
}
