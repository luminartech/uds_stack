//! The `DoIP` connection service: ISO 13400-2's own service interface, with no I/O.
//!
//! [`DiagnosticConnection`] is one connection as the layer above sees it: the
//! `DoIP_Data.request` primitive, and the confirm and indication primitives arriving as
//! [`ConnectionEvent`]s whose outcome is a [`DoIpResult`] (ISO 13400-2:2019 8.3). The
//! addressing model of a target is a [`TaType`], derived from the address by
//! [`LogicalAddress::default_ta_type`].
//!
//! [`DiagnosticEntity`] is a whole `DoIP` entity — every connection it has accepted —
//! as the layer above drives it: events tagged with the [`ConnectionId`] they arrived
//! on, requests routed by target address, and the close the diagnostic protocol
//! prescribes. [`EntityConfig`] is what an entity is told about its testers.
//!
//! Nothing here performs I/O or names a socket, so implementing these traits over an
//! application's own stack needs no dependency beyond this crate.
//!
//! # Time
//!
//! Every deadline here is on one clock, `embassy-time`'s: an `embassy_time::Instant`
//! in milliseconds (`Instant::as_millis`), truncated to 32 bits and wrapping. An
//! implementor and its caller each read that clock directly, which is why neither
//! trait reports the time, and why a deadline computed by the caller means the same
//! instant to the implementor. An implementation whose timers run on any other clock
//! does not meet these traits' contract.

use core::future::Future;

use crate::{LogicalAddress, TaType};

/// `DoIP_Result`: the outcome a confirm or indication primitive reports
/// (ISO 13400-2:2019 8.2.5).
///
/// Declared in the standard's order, which is normative: where several errors are
/// found at once, the one earliest in this list is reported. Exhaustive, as the
/// standard's list is: an edition that adds a value is a change every caller must
/// handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoIpResult {
    /// `DoIP_OK`: the service completed.
    Ok,
    /// `DoIP_HDR_ERROR`: the generic header was in error.
    HdrError,
    /// `DoIP_TIMEOUT_A`.
    TimeoutA,
    /// `DoIP_UNKNOWN_SA`: the source address is not known.
    UnknownSa,
    /// `DoIP_INVALID_SA`: the source address is not valid on this connection.
    ///
    /// The connection does not survive it: an entity rejecting a diagnostic message whose
    /// source address routing activation did not register on its socket also closes
    /// that socket (ISO 13400-2:2019 REQ 7.DoIP-070). Nothing more can be sent on
    /// the connection; a tester that wants to continue opens a new one and activates
    /// routing again.
    InvalidSa,
    /// `DoIP_UNKNOWN_TA`: the target address is not known.
    UnknownTa,
    /// `DoIP_MESSAGE_TOO_LARGE`: the message exceeds what can be carried.
    MessageTooLarge,
    /// `DoIP_OUT_OF_MEMORY`: the message exceeds the memory available for it.
    OutOfMemory,
    /// `DoIP_TARGET_UNREACHABLE`: the target cannot currently be reached.
    TargetUnreachable,
    /// `DoIP_NO_LINK`: there is no link.
    NoLink,
    /// `DoIP_NO_SOCKET`: there is no socket to carry the message.
    NoSocket,
    /// `DoIP_ERROR`: any other failure.
    Error,
}

/// What a [`DiagnosticConnection`] reports, or that the caller's deadline passed first.
///
/// **The lifetime is the caller's buffer, never the connection.** A PDU arrives as the
/// subslice of the buffer passed to [`DiagnosticConnection::next_event`] that it
/// occupies, so the connection is free to be used again while the PDU is still live —
/// which is what lets a server answer the request it has just received.
///
/// Exhaustive: an event added later is a change every caller must handle, so it stops
/// their build rather than reaching a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionEvent<'b> {
    /// `DoIP_Data.indication`: a diagnostic message arrived (ISO 13400-2:2019 8.3.3).
    ///
    /// Raised only for a message without error, so it carries no [`DoIpResult`]: an
    /// erroneous diagnostic message is ignored and raises no indication.
    Indication {
        /// The sender.
        sa: LogicalAddress,
        /// The target the sender addressed.
        ta: LogicalAddress,
        /// The target's addressing model:
        /// [`ta.default_ta_type()`](LogicalAddress::default_ta_type).
        ta_type: TaType,
        /// The PDU, in the caller's buffer.
        pdu: &'b [u8],
    },
    /// `DoIP_Data.indication` for a diagnostic message longer than the caller's buffer,
    /// truncated to what fit.
    ///
    /// A variant rather than a flag on [`Self::Indication`], so that a fragment cannot
    /// be destructured as a whole PDU.
    IndicationTruncated {
        /// The sender.
        sa: LogicalAddress,
        /// The target the sender addressed.
        ta: LogicalAddress,
        /// The target's addressing model:
        /// [`ta.default_ta_type()`](LogicalAddress::default_ta_type).
        ta_type: TaType,
        /// The leading bytes of the PDU that fit in the caller's buffer.
        pdu: &'b [u8],
        /// The whole PDU's length, from the message's header.
        length: usize,
    },
    /// `DoIP_Data.confirm`: a [`DiagnosticConnection::request`] completed or failed
    /// (ISO 13400-2:2019 8.3.2).
    ///
    /// A tester's request is confirmed by the entity's diagnostic message
    /// acknowledgement, positive or negative. An entity's is confirmed when it has been
    /// written, because a tester does not acknowledge diagnostic messages
    /// (ISO 13400-2:2019 9.5).
    Confirm {
        /// The source address of the confirmed request.
        sa: LogicalAddress,
        /// The target address of the confirmed request.
        ta: LogicalAddress,
        /// The target addressing model of the confirmed request.
        ta_type: TaType,
        /// The outcome; [`DoIpResult::Ok`] where the request completed.
        result: DoIpResult,
    },
    /// A valid message of a payload type this crate does not model, and its payload.
    ///
    /// Not a rejected message: a message found in error is not reported at all.
    Unmodelled {
        /// The message's payload type, as on the wire.
        payload_type: u16,
        /// The payload, in the caller's buffer.
        data: &'b [u8],
    },
    /// The connection is closed.
    ///
    /// Carries no reason: whether a close was one the diagnostic protocol prescribes is
    /// known to the layer that sent the message prescribing it, not to this one.
    Closed,
    /// The caller's deadline passed before anything arrived.
    Deadline,
}

/// One `DoIP` connection, as the layer above it uses it.
///
/// # Obligations on implementors
///
/// The signatures cannot state these, and the layer above relies on each:
///
/// - **[`Self::next_event`] is cancel-safe.** A caller races it against other work and
///   drops the losing future, often unpolled, and calls it again later — with a
///   different buffer. Dropping it must lose nothing. So a message is assembled in a
///   receive buffer the connection owns, sized to the largest message it accepts, and
///   copied into the caller's buffer only once complete; and anything the connection
///   writes from inside `next_event` (an acknowledgement, a routing activation
///   response, an alive check) is queued in the connection with its progress and
///   flushed first by the next call. Neither path may use a read or write that loses
///   progress when dropped, such as `read_exact` or `write_all`.
///
///   This holds only if the underlying socket's own reads and writes have no effect
///   when cancelled before completing. That is an obligation the implementor passes
///   on to whoever supplies the socket, and states.
/// - **Every accepted [`Self::request`] is followed by exactly one
///   [`ConnectionEvent::Confirm`]** with that request's addressing, including a failed
///   one where the connection closes before the request completed. The layer above
///   waits on that confirm.
pub trait DiagnosticConnection {
    /// What this connection's failures are. Never interpreted by the layer above, which
    /// can only report it.
    type Error: core::fmt::Debug;

    /// `DoIP_Data.request`: send `pdu` to `ta` (ISO 13400-2:2019 8.3.1).
    ///
    /// There is no source address: routing activation fixed it for the connection.
    /// Completion is not awaited here. It is reported by a later
    /// [`ConnectionEvent::Confirm`], because the confirm is what starts the layer
    /// above's response timer.
    ///
    /// # Arguments
    ///
    /// * `ta` - the target.
    /// * `ta_type` - the target's addressing model.
    /// * `pdu` - the PDU to send.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the request is not accepted; no confirm follows it.
    fn request(
        &mut self,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>>;

    /// The next event, written into `buf`, or [`ConnectionEvent::Deadline`] if
    /// `deadline_ms` passes first.
    ///
    /// Cancel-safe; see the trait's obligations.
    ///
    /// # Arguments
    ///
    /// * `buf` - where a PDU is delivered; the event borrows it.
    /// * `deadline_ms` - when to stop waiting, on the clock this module's time is
    ///   read from: `embassy-time`'s, in milliseconds, truncated to 32 bits and
    ///   wrapping. It may already have
    ///   passed. `None` waits for an event alone.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the connection fails other than by closing.
    fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline_ms: Option<u32>,
    ) -> impl Future<Output = Result<ConnectionEvent<'b>, Self::Error>>;
}

/// One connection in a [`DiagnosticEntity`]'s connection table.
///
/// Names one connection from the event that first reports it until the
/// [`EntityEvent::Closed`] reporting its end, or until [`DiagnosticEntity::close`] on it
/// returns. After that the same value may name a connection accepted later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u8);

impl ConnectionId {
    /// The connection in slot `index` of an entity's connection table.
    ///
    /// # Arguments
    ///
    /// * `index` - the slot, below [`DiagnosticEntity::CONNECTIONS`].
    #[must_use]
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    /// The connection's slot in the entity's connection table, which is below
    /// [`DiagnosticEntity::CONNECTIONS`].
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a [`DiagnosticEntity`] reports, or that the caller's deadline passed first.
///
/// [`ConnectionEvent`]'s variants, with the [`ConnectionId`] each arrived on wherever one
/// did. **The lifetime is the caller's buffer, never the entity**, exactly as for
/// [`ConnectionEvent`]. Exhaustive, as [`ConnectionEvent`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityEvent<'b> {
    /// `DoIP_Data.indication` on `connection`; see [`ConnectionEvent::Indication`].
    Indication {
        /// The connection the message arrived on.
        connection: ConnectionId,
        /// The sender, the source address routing activation registered on
        /// `connection`.
        sa: LogicalAddress,
        /// The target the sender addressed.
        ta: LogicalAddress,
        /// The target's addressing model:
        /// [`ta.default_ta_type()`](LogicalAddress::default_ta_type).
        ta_type: TaType,
        /// The PDU, in the caller's buffer.
        pdu: &'b [u8],
    },
    /// A diagnostic message longer than the caller's buffer; see
    /// [`ConnectionEvent::IndicationTruncated`].
    IndicationTruncated {
        /// The connection the message arrived on.
        connection: ConnectionId,
        /// The sender, the source address routing activation registered on
        /// `connection`.
        sa: LogicalAddress,
        /// The target the sender addressed.
        ta: LogicalAddress,
        /// The target's addressing model:
        /// [`ta.default_ta_type()`](LogicalAddress::default_ta_type).
        ta_type: TaType,
        /// The leading bytes of the PDU that fit in the caller's buffer.
        pdu: &'b [u8],
        /// The whole PDU's length, from the message's header.
        length: usize,
    },
    /// `DoIP_Data.confirm` for a [`DiagnosticEntity::request`]; see
    /// [`ConnectionEvent::Confirm`].
    ///
    /// Carries no [`ConnectionId`]: a request whose target no connection registered was
    /// carried by none, and is confirmed all the same.
    Confirm {
        /// The source address of the confirmed request.
        sa: LogicalAddress,
        /// The target address of the confirmed request.
        ta: LogicalAddress,
        /// The target addressing model of the confirmed request.
        ta_type: TaType,
        /// The outcome; [`DoIpResult::Ok`] where the request was written.
        result: DoIpResult,
    },
    /// A valid message of a payload type this crate does not model; see
    /// [`ConnectionEvent::Unmodelled`].
    Unmodelled {
        /// The connection the message arrived on.
        connection: ConnectionId,
        /// The message's payload type, as on the wire.
        payload_type: u16,
        /// The payload, in the caller's buffer.
        data: &'b [u8],
    },
    /// `connection` closed other than by [`DiagnosticEntity::close`]: the tester closed
    /// it, or the entity did on an error or a timeout.
    ///
    /// Reported only for a connection an earlier event named.
    Closed {
        /// The connection that closed.
        connection: ConnectionId,
    },
    /// The caller's deadline passed before anything arrived.
    Deadline,
}

/// A whole `DoIP` entity, as the layer above it drives it: every connection it has
/// accepted, behind one event stream.
///
/// The entity runs its own socket handling — accepting connections, routing
/// activation, alive checks and inactivity timeouts — inside [`Self::next_event`], and
/// reports none of it. An event names a connection only once routing is active on it,
/// so nothing arrives from a tester that has not activated routing.
///
/// # Obligations on implementors
///
/// [`DiagnosticConnection`]'s two, which the layer above relies on in the same way:
///
/// - **[`Self::next_event`] is cancel-safe**, on the same terms and at the same cost as
///   [`DiagnosticConnection::next_event`]: a receive buffer per connection, and writes
///   made inside `next_event` queued and flushed first by the next call.
/// - **Every accepted [`Self::request`] is followed by exactly one
///   [`EntityEvent::Confirm`]** with that request's addressing — including a request
///   whose target no connection registered, confirmed with [`DoIpResult::NoSocket`],
///   and one whose connection closed before it was written.
///
/// And two of its own:
///
/// - **The connection table changes only inside [`Self::next_event`] and
///   [`Self::close`]**, so a [`ConnectionId`] the caller holds keeps naming its
///   connection between the two calls that could end it.
/// - **[`Self::close`] is cancel-safe.** A caller dropped while closing calls `close`
///   again for the same connection, and that call finishes the close: the writes the
///   first call owed are made once, and the connection leaves the table once. The same
///   socket condition as for `next_event` applies.
pub trait DiagnosticEntity {
    /// What this entity's failures are. Never interpreted by the layer above, which can
    /// only report it.
    type Error: core::fmt::Debug;

    /// The size of this entity's connection table: every [`ConnectionId`] it reports
    /// has an index below it.
    ///
    /// Every `TCP_DATA` socket the entity supports counts, the reserve one included,
    /// so a conformant entity serving `n` testers at once declares `n + 1`
    /// (ISO 13400-2:2019 REQ 4.DoIP-002). Every established socket enters the
    /// connection table (REQ 3.DoIP-127), and routing is activated on the reserve
    /// socket when a tester returns without closing its old one (REQ 3.DoIP-092).
    const CONNECTIONS: usize;

    /// `DoIP_Data.request`: send `pdu` from `sa` to `ta` on the connection whose
    /// routing activation registered `ta` (ISO 13400-2:2019 8.3.1).
    ///
    /// Routing activation registers each source address on one connection only, so
    /// the target address alone chooses the connection. Completion is reported by a
    /// later [`EntityEvent::Confirm`]: a request whose `sa` is not one of the
    /// entity's own logical addresses is accepted, sends nothing, and is confirmed
    /// with [`DoIpResult::UnknownSa`].
    ///
    /// # Arguments
    ///
    /// * `sa` - the source, one of the entity's own logical addresses.
    /// * `ta` - the target, a tester's source address.
    /// * `ta_type` - the target's addressing model.
    /// * `pdu` - the PDU to send.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the request is not accepted; no confirm follows it. A
    /// target no connection registered is not an error.
    fn request(
        &mut self,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>>;

    /// The next event on any connection, written into `buf`, or
    /// [`EntityEvent::Deadline`] if `deadline_ms` passes first.
    ///
    /// Cancel-safe; see the trait's obligations.
    ///
    /// # Arguments
    ///
    /// * `buf` - where a PDU is delivered; the event borrows it.
    /// * `deadline_ms` - as for [`DiagnosticConnection::next_event`]. The entity's own
    ///   timers run whatever it is.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the entity as a whole fails. One connection failing is
    /// an [`EntityEvent::Closed`], not an error.
    fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline_ms: Option<u32>,
    ) -> impl Future<Output = Result<EntityEvent<'b>, Self::Error>>;

    /// Close `connection` in an orderly way, as ISO 14229-5:2022 REQ 7.11 requires of a
    /// server after a positive `ECUReset` response, and REQ 7.9 after a positive
    /// `DiagnosticSessionControl` response to a session change that leaves the software
    /// it is running.
    ///
    /// Everything requested on `connection` before this call is written first; the
    /// future completes once the close has been sent. The connection then leaves the
    /// table, no [`EntityEvent::Closed`] is reported for it, and a tester that wants to
    /// continue arrives as a new connection. Closing a [`ConnectionId`] that names no
    /// connection in the table — one that has already gone, or one the entity never
    /// issued — does nothing.
    ///
    /// This is the only close the caller can ask for. A close on an error is the
    /// entity's own decision, reported as [`EntityEvent::Closed`].
    ///
    /// Cancel-safe, as the trait's obligations require.
    ///
    /// # Arguments
    ///
    /// * `connection` - the connection to close.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the entity fails while closing. The connection has left the
    /// table either way.
    fn close(
        &mut self,
        connection: ConnectionId,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}

/// A tester address an [`EntityConfig`] was given that is outside the client range,
/// [`LogicalAddress::MIN_CLIENT_ADDRESS`]..=[`LogicalAddress::MAX_CLIENT_ADDRESS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "{address} is not a tester address: ISO 13400-2 Table 13 gives testers 0x0E00-0x0FFF"
)]
pub struct NotATesterAddress {
    /// The rejected address.
    pub address: LogicalAddress,
}

/// What a `DoIP` entity is told about its testers.
///
/// `TESTERS` is how many tester source addresses may activate routing; a routing
/// activation from any other is refused as an unknown source address.
///
/// # Examples
///
/// ```
/// use simple_doip::LogicalAddress;
/// use simple_doip::service::EntityConfig;
///
/// let config = EntityConfig::default();
/// assert!(config.accepts(LogicalAddress(0x0E00)));
/// assert!(!config.accepts(LogicalAddress(0x0E80)));
///
/// let config = EntityConfig::new([LogicalAddress(0x0E00), LogicalAddress(0x0E80)])?;
/// assert!(config.accepts(LogicalAddress(0x0E80)));
/// # Ok::<(), simple_doip::service::NotATesterAddress>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityConfig<const TESTERS: usize = 1> {
    accepted_testers: [LogicalAddress; TESTERS],
}

impl<const TESTERS: usize> EntityConfig<TESTERS> {
    /// A configuration accepting routing activation from `accepted_testers` only.
    ///
    /// # Arguments
    ///
    /// * `accepted_testers` - the tester source addresses that may activate routing.
    ///
    /// # Errors
    ///
    /// [`NotATesterAddress`] for the first of `accepted_testers` outside the client
    /// range.
    pub const fn new(
        accepted_testers: [LogicalAddress; TESTERS],
    ) -> Result<Self, NotATesterAddress> {
        const { assert!(TESTERS > 0, "an entity must accept at least one tester") };
        let mut i = 0;
        while i < TESTERS {
            let address = accepted_testers[i];
            if address.0 < LogicalAddress::MIN_CLIENT_ADDRESS.0
                || address.0 > LogicalAddress::MAX_CLIENT_ADDRESS.0
            {
                return Err(NotATesterAddress { address });
            }
            i += 1;
        }
        Ok(Self { accepted_testers })
    }

    /// Whether a tester with source address `sa` may activate routing.
    #[must_use]
    pub fn accepts(&self, sa: LogicalAddress) -> bool {
        self.accepted_testers.contains(&sa)
    }
}

impl Default for EntityConfig {
    /// Accepts the one tester `0x0E00`.
    fn default() -> Self {
        Self {
            accepted_testers: [LogicalAddress(0x0E00)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection that indicates one fixed request, then confirms what it is asked
    /// to send.
    #[derive(Debug, Default)]
    struct Echo {
        confirm: Option<(LogicalAddress, TaType)>,
        sent: [u8; 8],
        sent_len: usize,
    }

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "the fixture never awaits; it exists to prove the borrow shape"
    )]
    impl DiagnosticConnection for Echo {
        type Error = core::convert::Infallible;

        async fn request(
            &mut self,
            ta: LogicalAddress,
            ta_type: TaType,
            pdu: &[u8],
        ) -> Result<(), Self::Error> {
            let sent = self.sent.get_mut(..pdu.len()).unwrap();
            sent.copy_from_slice(pdu);
            self.sent_len = pdu.len();
            self.confirm = Some((ta, ta_type));
            Ok(())
        }

        async fn next_event<'b>(
            &mut self,
            buf: &'b mut [u8],
            _deadline_ms: Option<u32>,
        ) -> Result<ConnectionEvent<'b>, Self::Error> {
            if let Some((ta, ta_type)) = self.confirm.take() {
                return Ok(ConnectionEvent::Confirm {
                    sa: LogicalAddress(0x0001),
                    ta,
                    ta_type,
                    result: DoIpResult::Ok,
                });
            }
            let pdu = buf.get_mut(..2).unwrap();
            pdu.copy_from_slice(&[0x3E, 0x00]);
            Ok(ConnectionEvent::Indication {
                sa: LogicalAddress(0x0E00),
                ta: LogicalAddress(0x0001),
                ta_type: TaType::Physical,
                pdu,
            })
        }
    }

    /// The indicated PDU is still borrowed when the connection is used to answer it,
    /// which compiles only because the event borrows the buffer, not the connection.
    #[tokio::test]
    async fn an_indication_can_be_answered_while_it_is_borrowed() {
        let mut connection = Echo::default();
        let mut buf = [0u8; 8];

        let event = connection.next_event(&mut buf, None).await.unwrap();
        let ConnectionEvent::Indication { sa, pdu, .. } = event else {
            panic!("expected an indication, got {event:?}");
        };
        connection.request(sa, TaType::Physical, pdu).await.unwrap();
        assert_eq!(connection.sent.get(..connection.sent_len), Some(pdu));

        let mut buf = [0u8; 8];
        assert_eq!(
            connection.next_event(&mut buf, None).await.unwrap(),
            ConnectionEvent::Confirm {
                sa: LogicalAddress(0x0001),
                ta: LogicalAddress(0x0E00),
                ta_type: TaType::Physical,
                result: DoIpResult::Ok,
            }
        );
    }

    /// The sensor's configuration: routing activation from `0x0E00` and no other
    /// tester.
    #[test]
    fn the_default_entity_accepts_tester_0e00_only() {
        let config = EntityConfig::default();
        assert!(config.accepts(LogicalAddress(0x0E00)));
        assert!(!config.accepts(LogicalAddress(0x0E01)));
        assert!(!config.accepts(LogicalAddress(0x0FFF)));
    }

    /// A functional group address, like the `0xE400` this crate once offered as a tester
    /// address, is refused at construction, as is anything else outside the client
    /// range at either edge.
    #[test]
    fn an_entity_config_refuses_an_address_outside_the_client_range() {
        for address in [0xE400, 0x0DFF, 0x1000] {
            assert_eq!(
                EntityConfig::new([LogicalAddress(0x0E00), LogicalAddress(address)]),
                Err(NotATesterAddress {
                    address: LogicalAddress(address)
                }),
                "{address:#06X}"
            );
        }
        assert!(
            EntityConfig::new([LogicalAddress(0x0E00), LogicalAddress(0x0FFF)]).is_ok()
        );
    }
}
