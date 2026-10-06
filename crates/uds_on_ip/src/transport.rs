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
    Inbound, PduOutsideBuffer, classify, from_logical, target_of, to_doip_ta_type,
    to_logical,
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
/// entity's [`DiagnosticEntity::CONNECTIONS`], which [`DoIpTransport::new`] checks
/// at compile time.
///
/// # The prescribed close
///
/// Two messages are followed by a close: every positive `ECUReset` response
/// (REQ 7.11), and a message handed over with [`AfterSend::ServerLeaves`] — the
/// positive `DiagnosticSessionControl` response to a session change that leaves
/// the software the server is running, as an application entering its
/// bootloader does (REQ 7.9). Any other positive `DiagnosticSessionControl`
/// response closes nothing: the octet cannot say whether the change disconnects,
/// and only the server knows.
///
/// After sending one, the transport closes the connection the tester arrived on
/// with [`DiagnosticEntity::close`] once the entity confirms the message was
/// sent, and only then reports that confirmation. The close therefore follows
/// the response ("after sending") and precedes anything done on the
/// confirmation: `uds_services::Server` executes a session change there. It has
/// no hook yet that executes a reset on its confirmation, so an `ECUReset`
/// handler that resets on its own does so before the close. A message whose
/// confirmation fails closes nothing.
///
/// The close waits for every message sent to that tester to be confirmed, not
/// only the one that armed it, and holds back the last confirmation. That is the
/// armed message's own while nothing more is sent to the tester before it is
/// confirmed, which `uds_session` guarantees (`UDSS_LLR_0061`: no second
/// transmission on an addressing whose first is unconfirmed).
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
    unreported: Option<Confirmation>,
}

/// A tester with routing active on `connection`, and what its connection owes.
#[derive(Debug, Clone, Copy)]
struct Tester {
    connection: ConnectionId,
    address: LogicalAddress,
    owes: ConnectionAction,
    unconfirmed: u8,
    requested: Option<Ai>,
}

/// A prescribed close in progress, and the confirmation it holds back.
#[derive(Debug, Clone, Copy)]
struct Closing {
    connection: ConnectionId,
    confirmation: Confirmation,
}

#[derive(Debug, Clone, Copy)]
struct Confirmation {
    ai: Ai,
    result: SResult,
}

impl Confirmation {
    const fn event(self) -> TransportEvent<'static> {
        TransportEvent::DataConf {
            ai: self.ai,
            result: self.result,
        }
    }
}

impl<E, const MCTS: usize> core::fmt::Debug for DoIpTransport<E, MCTS> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpTransport")
            .field("entity", &"..")
            .field("reloads", &self.reloads)
            .field("outbound_max", &self.outbound_max)
            .field("testers", &self.testers)
            .field("closing", &self.closing)
            .field("unreported", &self.unreported)
            .finish()
    }
}

impl<E: DiagnosticEntity, const MCTS: usize> DoIpTransport<E, MCTS> {
    /// A transport over `entity`, loading the session layer's response timer
    /// with `reloads`.
    ///
    /// The peer's size bound starts unknown, because it is learned from the
    /// peer's entity status response rather than assumed. Does not compile
    /// where `MCTS` is below [`DiagnosticEntity::CONNECTIONS`], or zero.
    ///
    /// # Arguments
    ///
    /// * `entity` - the [`DiagnosticEntity`] whose connections this transport
    ///   serves.
    /// * `reloads` - the `tP6` pair; see
    ///   [`profile::bench_reloads`](crate::profile::bench_reloads) for values
    ///   suitable for a bench, and why they are not suitable for a vehicle.
    ///
    /// # Examples
    ///
    /// An entity with two connections does not fit the default table of one:
    ///
    /// ```compile_fail
    /// # use simple_doip::service::{ConnectionId, DiagnosticEntity, EntityEvent};
    /// # use simple_doip::{LogicalAddress, TaType};
    /// # use uds_on_ip::{DoIpTransport, profile::bench_reloads};
    /// struct TwoSockets;
    ///
    /// impl DiagnosticEntity for TwoSockets {
    ///     const CONNECTIONS: usize = 2;
    ///     // ...
    /// #   type Error = ();
    /// #   async fn request(
    /// #       &mut self,
    /// #       _: LogicalAddress,
    /// #       _: LogicalAddress,
    /// #       _: TaType,
    /// #       _: &[u8],
    /// #   ) -> Result<(), ()> {
    /// #       Ok(())
    /// #   }
    /// #   async fn next_event<'b>(
    /// #       &mut self,
    /// #       _: &'b mut [u8],
    /// #       _: Option<u32>,
    /// #   ) -> Result<EntityEvent<'b>, ()> {
    /// #       Ok(EntityEvent::Deadline)
    /// #   }
    /// #   async fn close(&mut self, _: ConnectionId) -> Result<(), ()> {
    /// #       Ok(())
    /// #   }
    /// }
    ///
    /// let transport: DoIpTransport<TwoSockets> = DoIpTransport::new(TwoSockets, bench_reloads());
    /// ```
    #[must_use]
    pub const fn new(entity: E, reloads: Reloads) -> Self {
        const { assert!(MCTS > 0, "a transport serves at least one connection") };
        const {
            assert!(
                MCTS >= E::CONNECTIONS,
                "MCTS is below the entity's connection table"
            );
        };
        Self {
            entity,
            reloads,
            outbound_max: None,
            testers: [None; MCTS],
            closing: None,
            unreported: None,
        }
    }
}

impl<E, const MCTS: usize> DoIpTransport<E, MCTS> {
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

    fn register(&mut self, connection: ConnectionId, address: LogicalAddress) {
        let index = connection.index();
        let Some(slot) = self.testers.get(index) else {
            return;
        };
        if slot.is_some_and(|tester| tester.address == address) {
            return;
        }
        for (other, tester) in self.testers.iter_mut().enumerate() {
            if other == index || tester.is_some_and(|tester| tester.address == address) {
                *tester = None;
            }
        }
        if let Some(slot) = self.testers.get_mut(index) {
            *slot = Some(Tester {
                connection,
                address,
                owes: ConnectionAction::Continue,
                unconfirmed: 0,
                requested: None,
            });
        }
    }

    fn forget(&mut self, connection: ConnectionId) -> Option<Tester> {
        self.testers
            .get_mut(connection.index())
            .and_then(Option::take)
    }

    fn record_send(&mut self, target: LogicalAddress, data: &[u8], after: AfterSend) {
        let first_octet = data.first().copied().unwrap_or_default();
        let action = after_sending(first_octet, after);
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
        if let Err(error) = closed {
            self.unreported = Some(closing.confirmation);
            return Err(Error::Entity(error));
        }
        Ok(closing.confirmation.event())
    }

    async fn confirm(
        &mut self,
        ai: Ai,
        result: SResult,
    ) -> Result<TransportEvent<'static>, Error<E::Error>> {
        let target = to_logical(ai.ta);
        let ai = self
            .tester_mut(target)
            .and_then(|tester| tester.requested)
            .unwrap_or(ai);
        let confirmation = Confirmation { ai, result };
        match self.record_confirm(target, result) {
            Some(connection) => {
                self.close(Closing {
                    connection,
                    confirmation,
                })
                .await
            }
            None => Ok(confirmation.event()),
        }
    }
}

impl<E: DiagnosticEntity, const MCTS: usize> UdsTransport for DoIpTransport<E, MCTS> {
    type Error = Error<E::Error>;

    /// `T_Data.req` as `DoIP_Data.request` (ISO 14229-5:2022 REQ 4.3 Table 4),
    /// routed by the target address to the connection that activated it. `ai`'s
    /// source address is the message's (REQ 4.4 Table 5), and the entity confirms
    /// one it does not own as failed.
    ///
    /// A positive `ECUReset` response, or a message `after` says the server leaves
    /// its running software on, arms the prescribed close described on
    /// [`DoIpTransport`]. The [`TransportEvent::DataConf`] that follows carries `ai`
    /// as given, whatever addressing the entity reports it with.
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
        after: AfterSend,
    ) -> Result<(), Self::Error> {
        let target = target_of(ai)?;
        self.entity
            .request(to_logical(ai.sa), target, to_doip_ta_type(ai.ta_type), data)
            .await
            .map_err(Error::Entity)?;
        self.record_send(target, data, after);
        if let Some(tester) = self.tester_mut(target) {
            tester.requested = Some(ai);
        }
        Ok(())
    }

    /// The next `T_Data.ind` or `T_Data.conf`, a closed connection, or
    /// [`TransportEvent::Deadline`] when `deadline` passes first.
    ///
    /// A message longer than `buffer` is [`TransportEvent::DataTooLong`] with `declared`
    /// always `Some`: `DoIP`'s generic header carries the length.
    /// [`TransportEvent::Closed`] names the tester whose connection closed, and is not
    /// reported for a connection no tester has sent a diagnostic message on. Its `expected`
    /// is true only for a tester owed a prescribed close. The prescribed close this
    /// transport makes itself is reported by no `Closed`, only by the
    /// [`TransportEvent::DataConf`] it held back. While that close is in progress,
    /// `deadline` is not honoured: the call returns once the entity has closed.
    /// Cancel-safe, as [`DiagnosticEntity`]'s obligations make its `next_event` and
    /// `close`.
    ///
    /// # Errors
    ///
    /// [`Error::Entity`] where the entity fails as a whole, and
    /// [`Error::PduOutsideBuffer`] where it reports a PDU outside `buffer`. Where it
    /// is the prescribed close
    /// that fails, the confirmation the close held back is not lost: the next call
    /// reports it.
    async fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, Self::Error> {
        if let Some(closing) = self.closing {
            return self.close(closing).await;
        }
        if let Some(confirmation) = self.unreported.take() {
            return Ok(confirmation.event());
        }
        let buffer_start = buffer.as_ptr().addr();
        let (connection, ai, at, declared) = loop {
            let event = self
                .entity
                .next_event(&mut *buffer, deadline.map(|at| at.0))
                .await
                .map_err(Error::Entity)?;
            let inbound = classify(event, buffer_start)
                .map_err(|PduOutsideBuffer| Error::PduOutsideBuffer)?;
            match inbound {
                None => {}
                Some(Inbound::Ind { connection, ai, at }) => {
                    break (connection, ai, at, None);
                }
                Some(Inbound::TooLong {
                    connection,
                    ai,
                    at,
                    declared,
                }) => break (connection, ai, at, Some(declared)),
                Some(Inbound::Conf { ai, result }) => {
                    return self.confirm(ai, result).await;
                }
                Some(Inbound::Closed { connection }) => {
                    if let Some(tester) = self.forget(connection) {
                        return Ok(TransportEvent::Closed {
                            peer: from_logical(tester.address),
                            expected: tester.owes.close_is_prescribed(),
                        });
                    }
                }
                Some(Inbound::Deadline) => return Ok(TransportEvent::Deadline),
            }
        };
        self.register(connection, to_logical(ai.sa));
        let buffer: &'b [u8] = buffer;
        let data = buffer.get(at).ok_or(Error::PduOutsideBuffer)?;
        Ok(match declared {
            None => TransportEvent::DataInd { ai, data },
            Some(declared) => TransportEvent::DataTooLong {
                ai,
                data,
                declared: Some(declared),
            },
        })
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
    use uds_services::{AfterSend, UdsTransport};
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
        const CONNECTIONS: usize = 1;
        async fn request(
            &mut self,
            _sa: LogicalAddress,
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
    fn serving_the_tester() -> DoIpTransport<Idle> {
        let mut t = DoIpTransport::new(Idle, bench_reloads());
        t.register(CONNECTION, TESTER);
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

    /// ISO 14229-5:2022 REQ 7.11: a positive `ECUReset` response owes a close of
    /// the connection its tester arrived on, once it is confirmed sent.
    #[test]
    fn a_confirmed_positive_reset_response_closes_its_connection() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// REQ 7.9's close is conditional on the session change disconnecting, which
    /// the response's octets cannot say, so a positive `DiagnosticSessionControl`
    /// response the server stays on closes nothing.
    #[test]
    fn a_positive_session_response_alone_closes_nothing() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x50, 0x03], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// REQ 7.9: a session change the server says leaves its running software owes
    /// the same close.
    #[test]
    fn a_confirmed_response_the_server_leaves_on_closes_its_connection() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x50, 0x02], AfterSend::ServerLeaves);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// REQ 7.11 keys the close on a *positive* response.
    #[test]
    fn a_negative_response_closes_nothing() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x7F, 0x11, 0x22], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A response that was not sent is not one the close may follow.
    #[test]
    fn a_failed_confirmation_closes_nothing_and_owes_nothing_after() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        let failed = SResult::Transport(TransportError(10));
        assert_eq!(t.record_confirm(TESTER, failed), None);

        t.record_send(TESTER, &[0x62, 0xF1, 0x90, 0x00], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A `0x78` still unconfirmed when the positive response is sent does not
    /// close the connection on its own confirmation: the close follows the last.
    #[test]
    fn the_close_waits_for_the_last_outstanding_confirmation() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x7F, 0x11, 0x78], AfterSend::Continue);
        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// Once owed, a close is not cancelled by a later send before it is made.
    #[test]
    fn a_later_send_does_not_cancel_an_owed_close() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        t.record_send(TESTER, &[0x7F, 0x22, 0x13], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(CONNECTION));
    }

    /// A message with no service identifier owes nothing, and indexing octet
    /// zero would panic on a length the peer controls.
    #[test]
    fn an_empty_message_owes_nothing() {
        let mut t = serving_the_tester();
        t.record_send(TESTER, &[], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }

    /// A response to a tester no connection carries has no connection to close.
    #[test]
    fn a_response_to_an_unknown_tester_closes_nothing() {
        let mut t = serving_the_tester();
        let stranger = LogicalAddress(0x0E80);
        t.record_send(stranger, &[0x51, 0x01], AfterSend::Continue);
        assert_eq!(t.record_confirm(stranger, SResult::Ok), None);
    }

    /// With two testers, each close goes to the connection its own tester arrived on.
    #[test]
    fn a_reset_response_closes_only_its_own_testers_connection() {
        let mut t = DoIpTransport::<Idle, 2>::new(Idle, bench_reloads());
        let other = LogicalAddress(0x0E80);
        let second = ConnectionId::new(1);
        t.register(CONNECTION, TESTER);
        t.register(second, other);

        t.record_send(other, &[0x51, 0x01], AfterSend::Continue);
        t.record_send(TESTER, &[0x62, 0xF1, 0x90, 0x00], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
        assert_eq!(t.record_confirm(other, SResult::Ok), Some(second));
    }

    /// Routing activation registers a tester's address on one connection only
    /// (ISO 13400-2:2019 9.6): a tester that reappears on another connection
    /// before the first is reported closed is the tester on the new one, and its
    /// close goes there.
    #[test]
    fn a_tester_reappearing_on_another_connection_is_closed_there() {
        let mut t = DoIpTransport::<Idle, 2>::new(Idle, bench_reloads());
        let second = ConnectionId::new(1);
        t.register(CONNECTION, TESTER);
        t.register(second, TESTER);

        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), Some(second));
        assert!(
            t.forget(CONNECTION).is_none(),
            "the first connection no longer holds the tester"
        );
    }

    /// A connection beyond the table, which only an entity breaking
    /// [`DiagnosticEntity::CONNECTIONS`] reports, is not remembered, so nothing is
    /// owed on it.
    #[test]
    fn a_connection_beyond_the_table_is_not_remembered() {
        let mut t = DoIpTransport::<Idle, 1>::new(Idle, bench_reloads());
        let second = ConnectionId::new(1);
        t.register(second, TESTER);
        t.record_send(TESTER, &[0x51, 0x01], AfterSend::Continue);
        assert_eq!(t.record_confirm(TESTER, SResult::Ok), None);
    }
}
