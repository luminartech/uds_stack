//! The `DoIP` connection service: ISO 13400-2's own service interface, with no I/O.
//!
//! [`DiagnosticConnection`] is one connection as the layer above sees it: the
//! `DoIP_Data.request` primitive, and the confirm and indication primitives arriving as
//! [`ConnectionEvent`]s whose outcome is a [`DoIpResult`] (ISO 13400-2:2019 8.3). The
//! addressing model of a target is a [`TaType`], derived from the address by
//! [`LogicalAddress::default_ta_type`].
//!
//! Nothing here performs I/O or names a socket, so implementing these traits over an
//! application's own stack needs no dependency beyond this crate.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// * `deadline_ms` - when to stop waiting, in milliseconds of the implementor's
    ///   monotonic clock, truncated to 32 bits and wrapping. It may already have
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
}
