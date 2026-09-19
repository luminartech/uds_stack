//! The crate's outward interface.
//!
//! [`DoIpTransport`] implements [`uds_services::UdsTransport`], which is the
//! whole of what this crate owes the stack. The trait and its
//! [`TransportEvent`](uds_services::TransportEvent) arrive from `uds_services`
//! rather than being mirrored here: a mirror is two vocabularies for one seam,
//! and the two drifted apart within a day of being written.
//!
//! Nothing here is shaped by `uds_services`: the trait's own test is that a
//! CAN binding implements the same methods, so a `DoIP`-shaped seam would be
//! the wrong seam.

use crate::error::Error;
use crate::mapping::target_of;
use uds_services::{TransportEvent, UdsTransport};
use uds_session::{Ai, Reloads, Timestamp};

/// ISO 14229-5 over `DoIP`.
///
/// `S` is the socket, so no runtime is named: this builds for a bare-metal
/// target as readily as for tokio, and an adapter for either is additive.
pub struct DoIpTransport<S> {
    #[expect(
        dead_code,
        reason = "read once t_data_req and next_event leave todo!()"
    )]
    socket: S,
    reloads: Reloads,
    outbound_max: Option<usize>,
}

// Written by hand rather than derived, and held by
// `tests::a_transport_over_an_opaque_socket_is_debug` rather than by this
// paragraph. `#[derive(Debug)]` would generate a conditional
// `impl<S: Debug> Debug for DoIpTransport<S>`, leaving `DoIpTransport<S>` with
// no `Debug` at all for any socket that is not itself `Debug` — the common case
// rather than the exception.
impl<S> core::fmt::Debug for DoIpTransport<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpTransport")
            .field("socket", &"..")
            .field("reloads", &self.reloads)
            .field("outbound_max", &self.outbound_max)
            .finish()
    }
}

impl<S> DoIpTransport<S> {
    /// A transport over `socket`, loading the session layer's response timer
    /// with `reloads`.
    ///
    /// See [`profile::bench_reloads`](crate::profile::bench_reloads) for values
    /// suitable for a bench, and for why they are not suitable for a vehicle.
    ///
    /// The peer's size bound starts unknown, because it is learned from the
    /// peer's entity status response rather than assumed.
    pub const fn new(socket: S, reloads: Reloads) -> Self {
        Self {
            socket,
            reloads,
            outbound_max: None,
        }
    }

    /// Record the peer's advertised *Max. data size*, learned from the peer's
    /// entity status response.
    ///
    /// A server typically has not requested one, which is why this stays
    /// `None` and `responseTooLong` is then unreachable rather than fabricated.
    ///
    /// # There is no inbound counterpart
    ///
    /// This entity's *own* MDS is not this crate's to hold. `simple_doip`
    /// answers the ISO 13400-2:2019 Table 11 entity status request itself and
    /// fills `max_data_size` from its own receive capacity, so a number stored
    /// here would reach no response. `uds_services` reached the same conclusion
    /// from the other side and removed `inbound_max` from
    /// [`UdsTransport`]: the driver derives the figure from the services it
    /// assembled and nothing on the seam ever asked for it.
    ///
    /// An `inbound_max`/`set_inbound_max` pair used to sit here, with a
    /// paragraph explaining how the advertised number and the buffer that must
    /// hold a request were kept in agreement. Nothing enforced that agreement,
    /// and with the producer and the consumer both elsewhere there was nothing
    /// for it to agree with.
    pub fn set_outbound_max(&mut self, max: Option<usize>) {
        self.outbound_max = max;
    }
}

impl<S> UdsTransport for DoIpTransport<S> {
    type Error = Error;

    /// `T_Data.req` — map a `T_PDU` onto a `DoIP` diagnostic message and send it.
    ///
    /// # Errors
    ///
    /// [`Error::Mapping`] if the addressing cannot be carried: the two remote
    /// message types have no `DoIP` representation.
    ///
    /// A *socket* failure has no variant yet — see [`Self::next_event`].
    #[expect(
        unused_variables,
        reason = "data is unused until t_data_req's body replaces the todo!() below"
    )]
    async fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> Result<(), Error> {
        let _target = target_of(ai)?;
        todo!("REQ 4.3 Table 4 — send as a DoIP diagnostic message")
    }

    /// The next inbound event, or
    /// [`TransportEvent::Deadline`](uds_services::TransportEvent::Deadline)
    /// when `deadline` passes first.
    ///
    /// An inbound payload is written into `buffer` and reported as the subslice
    /// it occupies, so the event borrows the caller's buffer and never `self` —
    /// which is what lets the driver answer a request while its bytes are still
    /// live. `deadline` is the session layer's `next_deadline`, so this
    /// transport never invents one.
    ///
    /// # A message longer than `buffer`
    ///
    /// Reported as
    /// [`TransportEvent::DataTooLong`](uds_services::TransportEvent::DataTooLong),
    /// never as a `DataInd` whose data happens to fill `buffer`. The caller must
    /// be able to tell a whole message from the front of a longer one: the first
    /// is dispatchable and the second is only classifiable.
    ///
    /// `declared` is always `Some` here. ISO 13400-2's generic header carries
    /// the payload length and it is read before the payload, so truncation is
    /// decided before there is a whole message to classify — never by comparing
    /// what arrived against `buffer.len()` afterwards.
    ///
    /// A driver serving a request offers only its small concurrent buffer, as
    /// ISO 14229-1 8.7.6 requires for the functionally addressed
    /// `TesterPresent` and the `0x00`–`0x0F` range. Truncation is the normal
    /// outcome in that window, not a fault: 8.7.6 owes that request
    /// `busyRepeatRequest` (0x21) regardless, and composing one needs the
    /// service identifier and the addressing, both of which this carries.
    ///
    /// # Errors
    ///
    /// [`Error::Wire`] if a `DoIP` message could not be decoded.
    ///
    /// # A socket failure has nowhere to go yet
    ///
    /// `S` carries no bound, so a socket has no error type for [`Error`] to
    /// compose, and [`Error::Wire`] is explicitly not it. There is no such
    /// bound available to take: `automotive-wire-codec`'s `Sink` is
    /// synchronous, has no read side, and its `WriteError::Io` carries no
    /// detail by design, while every async surface in `simple_doip` sits
    /// behind its `codec` feature, which requires `std` and a runtime. No
    /// `no_std` async socket seam exists anywhere in this stack.
    ///
    /// [`UdsTransport::Error`] is this crate's to name, so the shape is settled
    /// and only the bound is open. Until it lands, the failure this method is
    /// most likely to have is unrepresentable.
    #[expect(
        unused_variables,
        reason = "buffer and deadline are unused until next_event's body replaces the todo!()"
    )]
    async fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, Error> {
        todo!("read a DoIP message, mapping::classify it, translate it onto the seam")
    }

    /// The largest `A_PDU` the peer will accept, where it has advertised one.
    ///
    /// ISO 13400-2:2019 Table 11 — support for *Max. data size* is
    /// **optional**, so `None` is conformant. MDS is defined as the maximum
    /// size of one logical **request** an entity can process, so a server
    /// asking what it may send is asking about the client.
    fn outbound_max(&self) -> Option<usize> {
        self.outbound_max
    }

    /// The `tP_Client` reload pair this transport dictates.
    ///
    /// What this transport dictates is *which* pair: `DoIP` has no
    /// `T_DataSOM.ind`, so ISO 14229-2:2021 REQ 5.11 gives it `tP6` rather than
    /// `tP2`. The session layer does not distinguish the two, so the choice
    /// lives in these values and nowhere else.
    fn channel_timing(&self) -> Reloads {
        self.reloads
    }

    /// The current time.
    ///
    /// [`Timestamp`] rather than a bare `u32`: it is a newtype over exactly
    /// that `u32`, and it carries `interval_since`, the modulo-2³² subtraction
    /// `UDSS_LLR_0019` requires. The value `uds_session`'s `next_deadline`
    /// returns can then be handed straight back to [`Self::next_event`] with no
    /// arithmetic on either side of the seam.
    fn now(&self) -> Timestamp {
        todo!("the clock belongs to whatever S is; see the missing-bound note on next_event")
    }
}

#[cfg(test)]
mod tests {
    use super::DoIpTransport;
    use crate::profile::bench_reloads;
    use uds_services::UdsTransport;

    /// ISO 13400-2:2019 Table 11 marks *Max. data size* support **optional**, so
    /// a conformant `DoIP` entity need not advertise one and `None` is a correct
    /// answer rather than a defect.
    ///
    /// A bound is reported only where one was learned, never fabricated: a
    /// response sink bounded at an invented number would reject responses the
    /// peer would have accepted.
    #[test]
    fn an_unadvertised_max_data_size_is_none_not_a_guess() {
        let t = DoIpTransport::new((), bench_reloads());
        assert_eq!(t.outbound_max(), None);
    }

    /// What the peer advertised is what `outbound_max` reports.
    #[test]
    fn the_peers_bound_is_reported_once_it_is_learned() {
        let mut t = DoIpTransport::new((), bench_reloads());
        t.set_outbound_max(Some(4096));
        assert_eq!(t.outbound_max(), Some(4096));
    }

    /// The reload pair crosses the seam unchanged, so `DoIP`'s choice of `tP6`
    /// is what the session layer actually loads.
    #[test]
    fn the_reloads_reach_the_seam_unchanged() {
        let t = DoIpTransport::new((), bench_reloads());
        assert_eq!(t.channel_timing(), bench_reloads());
    }

    /// The hand-written `Debug` covers a socket that is not itself `Debug`.
    ///
    /// This is the guard for that choice; the note above the impl only explains
    /// it. A `#[derive(Debug)]` generates `impl<S: Debug>`, under which
    /// `DoIpTransport<OpaqueSocket>` has no `Debug` at all and this stops
    /// compiling. Verified by deriving it and watching this line fail.
    #[test]
    fn a_transport_over_an_opaque_socket_is_debug() {
        struct OpaqueSocket;
        const fn assert_debug<T: core::fmt::Debug>() {}
        assert_debug::<DoIpTransport<OpaqueSocket>>();
    }

    /// `Timestamp` is what makes the deadline exchangeable across the seam
    /// without arithmetic: the value `uds_session` reports as a next deadline
    /// goes straight back into `next_event`, and `interval_since` carries
    /// `UDSS_LLR_0019`'s modulo-2³² subtraction so a wrap is not a special case
    /// at either end.
    #[test]
    fn a_deadline_survives_the_wrap_it_is_typed_for() {
        let before_wrap = uds_session::Timestamp(u32::MAX - 10);
        let after_wrap = uds_session::Timestamp(5);
        assert_eq!(after_wrap.interval_since(before_wrap), 16);
    }
}
