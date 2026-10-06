//! The crate's outward interface.
//!
//! [`DoIpTransport`] implements [`uds_services::UdsTransport`], which is the
//! whole of what this crate owes the stack. The trait and its
//! [`TransportEvent`] arrive from `uds_services`
//! rather than being mirrored here: a mirror is two vocabularies for one seam,
//! and the two drifted apart within a day of being written.
//!
//! Nothing here is shaped by `uds_services`: the trait's own test is that a
//! CAN binding implements the same methods, so a `DoIP`-shaped seam would be
//! the wrong seam.

use crate::error::Error;
use crate::mapping::{
    Inbound, PduOutsideBuffer, classify, target_of, to_doip_ta_type, to_logical,
};
use crate::profile::{ConnectionAction, after_sending};
use simple_doip::LogicalAddress;
use simple_doip::service::{ConnectionId, DiagnosticEntity};
use uds_services::{AfterSend, TransportEvent, UdsTransport};
use uds_session::{Ai, Reloads, SResult, Timestamp};

/// ISO 14229-5 over a `DoIP` entity: the server side of `UDSonIP`.
///
/// One transport drives the whole [`DiagnosticEntity`] — every connection it has
/// accepted — and feeds one `uds_services::Server`, because the session, the
/// security state and `tS3_Server` are the server's rather than a connection's.
/// Responses are routed by their target address, which routing activation
/// registers on one connection only.
///
/// `MCTS` sizes the table in which the transport remembers which tester arrived
/// on which connection, so that it can close the right one when
/// ISO 14229-5:2022 REQ 7.9 or REQ 7.11 requires it. It must be at least the
/// entity's own maximum number of concurrent connections; a connection beyond it
/// is [`Error::ConnectionOutsideTable`].
///
/// # The prescribed close
///
/// After sending a positive `DiagnosticSessionControl` or `ECUReset` response,
/// the transport closes the connection the tester arrived on with
/// [`DiagnosticEntity::close`] once the entity confirms the response was sent,
/// and only then reports that confirmation. The close therefore follows the
/// response (REQ 7.9, REQ 7.11: "after sending") and precedes the service's
/// execution, which the driver begins on the confirmation. A response whose
/// confirmation fails closes nothing.
///
/// # Integrating on bare metal
///
/// [`uds_services::Server::step`] takes `&mut self`, and this stack forbids
/// `unsafe`, so a server built in place in a `static` is reached in one of two
/// ways. Where one task owns it, `static_cell::ConstStaticCell` yields the
/// `&'static mut` once, as `uds_services::Server::new` shows. Where anything
/// else must reach it too — an interrupt handler, a second task — it lives in a
/// `critical_section::Mutex<core::cell::RefCell<..>>`, and the integrator
/// supplies a `critical-section` implementation for the target.
///
/// Time is `embassy-time`'s, read by this transport and by the entity alike, so
/// the integrator links one `embassy-time` driver for the target — and, for an
/// entity that waits on `embassy-time` timers, a timer queue.
pub struct DoIpTransport<E, const MCTS: usize = 1> {
    entity: E,
    reloads: Reloads,
    outbound_max: Option<usize>,
    testers: [Option<Tester>; MCTS],
    closing: Option<Closing>,
}

/// A tester with routing active on `connection`, and what its connection owes.
#[derive(Debug, Clone, Copy)]
struct Tester {
    connection: ConnectionId,
    address: LogicalAddress,
    owes: ConnectionAction,
    unconfirmed: u8,
}

/// A prescribed close in progress, and the confirmation it holds back.
#[derive(Debug, Clone, Copy)]
struct Closing {
    connection: ConnectionId,
    ai: Ai,
    result: SResult,
}

impl<E, const MCTS: usize> core::fmt::Debug for DoIpTransport<E, MCTS> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpTransport")
            .field("entity", &"..")
            .field("reloads", &self.reloads)
            .field("outbound_max", &self.outbound_max)
            .field("testers", &self.testers)
            .field("closing", &self.closing)
            .finish()
    }
}

impl<E, const MCTS: usize> DoIpTransport<E, MCTS> {
    /// A transport over `entity`, loading the session layer's response timer
    /// with `reloads`.
    ///
    /// The peer's size bound starts unknown, because it is learned from the
    /// peer's entity status response rather than assumed.
    ///
    /// # Arguments
    ///
    /// * `entity` - the [`DiagnosticEntity`] whose connections this transport
    ///   serves.
    /// * `reloads` - the `tP6` pair; see
    ///   [`profile::bench_reloads`](crate::profile::bench_reloads) for values
    ///   suitable for a bench, and why they are not suitable for a vehicle.
    #[must_use]
    pub const fn new(entity: E, reloads: Reloads) -> Self {
        Self {
            entity,
            reloads,
            outbound_max: None,
            testers: [None; MCTS],
            closing: None,
        }
    }

    /// The entity, for inspection between events.
    #[must_use]
    pub const fn entity(&self) -> &E {
        &self.entity
    }

    /// Record the peer's advertised *Max. data size*, learned from the peer's
    /// entity status response.
    ///
    /// A server typically has not requested one, which is why this stays
    /// `None` and `responseTooLong` is then unreachable rather than fabricated.
    /// This entity's *own* MDS is not this crate's to hold: `simple_doip`
    /// answers the ISO 13400-2:2019 Table 11 entity status request itself.
    pub fn set_outbound_max(&mut self, max: Option<usize>) {
        self.outbound_max = max;
    }

    fn tester_mut(&mut self, address: LogicalAddress) -> Option<&mut Tester> {
        self.testers
            .iter_mut()
            .flatten()
            .find(|tester| tester.address == address)
    }

    fn register(
        &mut self,
        connection: ConnectionId,
        address: LogicalAddress,
    ) -> Result<(), ConnectionId> {
        let slot = self.testers.get_mut(connection.index()).ok_or(connection)?;
        if !slot.is_some_and(|tester| tester.address == address) {
            *slot = Some(Tester {
                connection,
                address,
                owes: ConnectionAction::Continue,
                unconfirmed: 0,
            });
        }
        Ok(())
    }

    fn forget(&mut self, connection: ConnectionId) -> Option<Tester> {
        self.testers
            .get_mut(connection.index())
            .and_then(Option::take)
    }

    fn record_send(&mut self, target: LogicalAddress, data: &[u8]) {
        let action = data
            .first()
            .map_or(ConnectionAction::Continue, |octet| after_sending(*octet));
        if let Some(tester) = self.tester_mut(target) {
            if action == ConnectionAction::InitiateClose
                || tester.owes != ConnectionAction::InitiateClose
            {
                tester.owes = action;
            }
            tester.unconfirmed = tester.unconfirmed.saturating_add(1);
        }
    }

    /// The connection to close, where this confirmation is the last one a
    /// tester owed a prescribed close was waiting on.
    fn record_confirm(
        &mut self,
        target: LogicalAddress,
        result: SResult,
    ) -> Option<ConnectionId> {
        let tester = self.tester_mut(target)?;
        tester.unconfirmed = tester.unconfirmed.saturating_sub(1);
        if tester.unconfirmed > 0 {
            return None;
        }
        let owed = core::mem::replace(&mut tester.owes, ConnectionAction::Continue);
        (owed == ConnectionAction::InitiateClose && result == SResult::Ok)
            .then_some(tester.connection)
    }
}

impl<E: DiagnosticEntity, const MCTS: usize> DoIpTransport<E, MCTS> {
    async fn close(
        &mut self,
        closing: Closing,
    ) -> Result<TransportEvent<'static>, Error<E::Error>> {
        self.closing = Some(closing);
        self.forget(closing.connection);
        let closed = self.entity.close(closing.connection).await;
        self.closing = None;
        closed.map_err(Error::Entity)?;
        Ok(TransportEvent::DataConf {
            ai: closing.ai,
            result: closing.result,
        })
    }
}

impl<E: DiagnosticEntity, const MCTS: usize> UdsTransport for DoIpTransport<E, MCTS> {
    type Error = Error<E::Error>;

    /// `T_Data.req` as `DoIP_Data.request` (ISO 14229-5:2022 REQ 4.3 Table 4),
    /// routed by the target address to the connection that activated it.
    ///
    /// A positive `DiagnosticSessionControl` or `ECUReset` response arms the
    /// prescribed close described on [`DoIpTransport`].
    ///
    /// # Errors
    ///
    /// [`Error::Mapping`] if the addressing cannot be carried: the two remote
    /// message types have no `DoIP` representation. [`Error::Entity`] if the
    /// entity does not accept the request.
    async fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        _after: AfterSend,
    ) -> Result<(), Self::Error> {
        let target = target_of(ai)?;
        self.entity
            .request(target, to_doip_ta_type(ai.ta_type), data)
            .await
            .map_err(Error::Entity)?;
        self.record_send(target, data);
        Ok(())
    }

    /// The next `T_Data.ind` or `T_Data.conf`, a closed connection, or
    /// [`TransportEvent::Deadline`] when `deadline` passes first.
    ///
    /// A message longer than `buffer` is [`TransportEvent::DataTooLong`] with
    /// `declared` always `Some`: `DoIP`'s generic header carries the length.
    /// [`TransportEvent::Closed`]'s `expected` is true only for a tester owed a
    /// prescribed close. Cancel-safe, provided the entity's `next_event` and
    /// `close` are.
    ///
    /// # Errors
    ///
    /// [`Error::Entity`] where the entity fails as a whole, and
    /// [`Error::ConnectionOutsideTable`] or [`Error::PduOutsideBuffer`] where it
    /// breaks its contract with this transport.
    async fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, Self::Error> {
        if let Some(closing) = self.closing {
            return self.close(closing).await;
        }
        let buffer_start = buffer.as_ptr().addr();
        let inbound = loop {
            let event = self
                .entity
                .next_event(&mut *buffer, deadline.map(|at| at.0))
                .await
                .map_err(Error::Entity)?;
            if let Some(inbound) = classify(event, buffer_start)
                .map_err(|PduOutsideBuffer| Error::PduOutsideBuffer)?
            {
                break inbound;
            }
        };
        let buffer: &'b [u8] = buffer;
        let outside_table = |connection| Error::ConnectionOutsideTable {
            connection,
            capacity: MCTS,
        };
        match inbound {
            Inbound::Ind { connection, ai, at } => {
                self.register(connection, to_logical(ai.sa))
                    .map_err(outside_table)?;
                let data = buffer.get(at).ok_or(Error::PduOutsideBuffer)?;
                Ok(TransportEvent::DataInd { ai, data })
            }
            Inbound::TooLong {
                connection,
                ai,
                at,
                declared,
            } => {
                self.register(connection, to_logical(ai.sa))
                    .map_err(outside_table)?;
                let data = buffer.get(at).ok_or(Error::PduOutsideBuffer)?;
                Ok(TransportEvent::DataTooLong {
                    ai,
                    data,
                    declared: Some(declared),
                })
            }
            Inbound::Conf { ai, result } => {
                match self.record_confirm(to_logical(ai.ta), result) {
                    Some(connection) => {
                        self.close(Closing {
                            connection,
                            ai,
                            result,
                        })
                        .await
                    }
                    None => Ok(TransportEvent::DataConf { ai, result }),
                }
            }
            Inbound::Closed { connection } => Ok(TransportEvent::Closed {
                expected: self
                    .forget(connection)
                    .is_some_and(|tester| tester.owes.close_is_prescribed()),
            }),
            Inbound::Deadline => Ok(TransportEvent::Deadline),
        }
    }

    /// The largest `A_PDU` the peer will accept, where it has advertised one.
    ///
    /// ISO 13400-2:2019 Table 11 — support for *Max. data size* is
    /// **optional**, so `None` is conformant.
    fn outbound_max(&self) -> Option<usize> {
        self.outbound_max
    }

    /// The `tP_Client` reload pair this transport dictates: `DoIP` has no
    /// `T_DataSOM.ind`, so ISO 14229-2:2021 REQ 5.11 gives it `tP6` rather than
    /// `tP2`.
    fn channel_timing(&self) -> Reloads {
        self.reloads
    }

    /// `embassy-time`'s clock in milliseconds, truncated to 32 bits: the clock
    /// [`simple_doip::service`]'s deadlines are on, so a deadline the session
    /// layer computes from this reaches the entity unconverted.
    fn now(&self) -> Timestamp {
        let [b0, b1, b2, b3, ..] = embassy_time::Instant::now().as_millis().to_le_bytes();
        Timestamp(u32::from_le_bytes([b0, b1, b2, b3]))
    }
}

#[cfg(test)]
mod tests {
    use super::DoIpTransport;
    use crate::profile::bench_reloads;
    use simple_doip::service::{ConnectionId, DiagnosticEntity, EntityEvent};
    use simple_doip::{LogicalAddress, TaType};
    use uds_services::UdsTransport;
    use uds_session::{SResult, TransportError};

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);
    const CONNECTION: ConnectionId = ConnectionId::new(0);

    /// An entity on which nothing ever happens.
    #[derive(Debug)]
    struct Idle;

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "nothing happens, so nothing is awaited"
    )]
    impl DiagnosticEntity for Idle {
        type Error = core::convert::Infallible;
        async fn request(
            &mut self,
            _ta: LogicalAddress,
            _ta_type: TaType,
            _pdu: &[u8],
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        async fn next_event<'b>(
            &mut self,
            _buf: &'b mut [u8],
            _deadline_ms: Option<u32>,
        ) -> Result<EntityEvent<'b>, Self::Error> {
            Ok(EntityEvent::Deadline)
        }
        async fn close(&mut self, _connection: ConnectionId) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// A transport with tester `0x0E00` registered on connection 0.
    fn serving_the_tester() -> DoIpTransport<()> {
        let mut t = DoIpTransport::new((), bench_reloads());
        assert_eq!(t.register(CONNECTION, TESTER), Ok(()));
        t
    }

    /// ISO 13400-2:2019 Table 11 marks *Max. data size* support **optional**, so
    /// a conformant `DoIP` entity need not advertise one and `None` is a correct
    /// answer rather than a defect.
    #[test]
    fn an_unadvertised_max_data_size_is_none_not_a_guess() {
        let t = DoIpTransport::<_>::new(Idle, bench_reloads());
        assert_eq!(t.outbound_max(), None);
    }

    /// What the peer advertised is what `outbound_max` reports.
    #[test]
    fn the_peers_bound_is_reported_once_it_is_learned() {
        let mut t = DoIpTransport::<_>::new(Idle, bench_reloads());
        t.set_outbound_max(Some(4096));
        assert_eq!(t.outbound_max(), Some(4096));
    }

    /// The reload pair crosses the seam unchanged, so `DoIP`'s choice of `tP6`
    /// is what the session layer actually loads.
    #[test]
    fn the_reloads_reach_the_seam_unchanged() {
        let t = DoIpTransport::<_>::new(Idle, bench_reloads());
        assert_eq!(t.channel_timing(), bench_reloads());
    }

    /// The hand-written `Debug` covers an entity that is not itself `Debug`; a
    /// derived one would not.
    #[test]
    fn a_transport_over_an_opaque_entity_is_debug() {
        struct OpaqueEntity;
        const fn assert_debug<T: core::fmt::Debug>() {}
        assert_debug::<DoIpTransport<OpaqueEntity>>();
    }

    /// ISO 14229-5:2022 REQ 7.9: a positive `DiagnosticSessionControl` response
    /// owes a close of the connection its tester arrived on, once it is
    /// confirmed sent.
    #[test]
    fn a_confirmed_positive_session_response_closes_its_connection() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x50, 0x03]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// REQ 7.11: so does a positive `ECUReset` response.
    #[test]
    fn a_confirmed_positive_reset_response_closes_its_connection() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x51, 0x01]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// REQ 7.9 and REQ 7.11 key the close on a *positive* response.
    #[test]
    fn a_negative_response_closes_nothing() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x7F, 0x10, 0x22]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A response that was not sent is not one the close may follow.
    #[test]
    fn a_failed_confirmation_closes_nothing_and_owes_nothing_after() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x50, 0x03]);
        let failed = SResult::Transport(TransportError(10));
        assert_eq!(t.record_confirm(TESTER, failed), None);

        t.record_send(TESTER, &[0x62, 0xF1, 0x90, 0x00]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A `0x78` still unconfirmed when the positive response is sent does not
    /// close the connection on its own confirmation: the close follows the last.
    #[test]
    fn the_close_waits_for_the_last_outstanding_confirmation() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x7F, 0x10, 0x78]);
        t.record_send(TESTER, &[0x50, 0x02]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// Once owed, a close is not cancelled by a later send before it is made.
    #[test]
    fn a_later_send_does_not_cancel_an_owed_close() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x50, 0x03]);
        t.record_send(TESTER, &[0x7F, 0x22, 0x13]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// A message with no service identifier owes nothing, and indexing octet
    /// zero would panic on a length the peer controls.
    #[test]
    fn an_empty_message_owes_nothing() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[]);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A response to a tester no connection carries has no connection to close.
    #[test]
    fn a_response_to_an_unknown_tester_closes_nothing() {
        let mut t = serving_the_tester();
        let stranger = LogicalAddress(0x0E80);
        t.record_send(stranger, &[0x50, 0x03]);
        assert_eq!(t.record_confirm(stranger, SResult::Ok), None);
    }

    /// A connection beyond `MCTS` cannot be remembered, and says so.
    #[test]
    fn a_connection_beyond_the_table_is_refused() {
        let mut t = DoIpTransport::<(), 1>::new((), bench_reloads());
        let second = ConnectionId::new(1);
        assert_eq!(t.register(second, TESTER), Err(second));
    }

    /// The value `uds_session` reports as a next deadline goes straight back into
    /// `next_event`, and `interval_since` carries `UDSS_LLR_0019`'s modulo-2³²
    /// subtraction, so a wrap is not a special case at either end.
    #[test]
    fn a_deadline_survives_the_wrap_it_is_typed_for() {
        let before_wrap = uds_session::Timestamp(u32::MAX - 10);
        let after_wrap = uds_session::Timestamp(5);
        assert_eq!(after_wrap.interval_since(before_wrap), 16);
    }
}
