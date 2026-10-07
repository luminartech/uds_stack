//! The handlers a received frame meets before the socket handler: Figure 16's generic
//! header handler, and Figure 17's diagnostic message handler.

use embassy_time::Instant;

use super::table::{Activation, AliveCheck, Open, Phase, Slot};
use super::{CLOSE_LIMIT, EntityAddress, GENERAL_INACTIVITY, expiry};
use crate::messages::{
    DiagnosticNackCode, Header, Message, MessageError, NackCode, Payload, PayloadType,
    ProtocolVersion,
};
use crate::stream::rx::{Next, RxBuffer};
use crate::stream::tx::TxQueue;
use crate::{LogicalAddress, TaType};

/// The encoded sizes of the entity's answers, for which room is found before the frame
/// they answer is taken.
pub(super) const HEADER_NACK: usize = Header::SIZE + 1;
pub(super) const DIAGNOSTIC_ACK: usize = Header::SIZE + 5;
pub(super) const ROUTING_ACTIVATION_RESPONSE: usize = Header::SIZE + 9;
pub(super) const ALIVE_CHECK_REQUEST: usize = Header::SIZE;

/// What became of the frame at the front of a slot's receive buffer.
pub(super) enum Handled {
    /// No whole header or frame yet, or no room to answer it.
    Waiting,
    /// Answered, discarded or acted on within the slot.
    Done,
    /// A routing activation request for the socket handler, with room kept for its
    /// response.
    Activation(Activation),
    Indication {
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        copied: usize,
        length: usize,
    },
}

/// Handles the frame at the front of `slot`'s receive buffer.
///
/// `limit` bounds a frame on a socket that has not activated routing, which may have
/// to move into the reserve. Routing activation requests wait while `arbitrating`.
pub(super) fn handle<S, const CAP: usize>(
    slot: &mut Slot<S, CAP>,
    address: EntityAddress,
    limit: usize,
    arbitrating: bool,
    buf: &mut [u8],
    now: Instant,
) -> Handled {
    let close_by = expiry(now, CLOSE_LIMIT);
    let Slot { open, rx, tx, .. } = slot;
    let Some(open) = open.as_mut() else {
        return Handled::Waiting;
    };
    let registered = match open.phase {
        Phase::Finalizing { .. } => return Handled::Waiting,
        Phase::Registered { sa, .. } => Some(sa),
        Phase::Initialized => None,
    };
    if let Err(handled) = check_header(open, rx, tx, registered.is_some(), limit, close_by)
    {
        return handled;
    }
    let Ok(Next::Frame(frame, consumed)) = rx.next() else {
        return Handled::Waiting;
    };
    let payload_type = frame.header.payload_type;
    open.version = frame.header.protocol_version;
    let handled = match (payload_type, Payload::decode(frame.payload, payload_type)) {
        (_, Ok(Payload::RoutingActivationRequest(request))) => {
            if registered.is_none() && !open.activation_received {
                open.activation_received = true;
                open.deadline = expiry(now, GENERAL_INACTIVITY);
            }
            if arbitrating || !tx.has_room_for(ROUTING_ACTIVATION_RESPONSE) {
                return Handled::Waiting;
            }
            Handled::Activation(Activation {
                sa: request.source_address,
                activation_type: request.activation_type,
            })
        }
        (_, Ok(Payload::AliveCheckResponse(_))) => {
            if let Phase::Registered { alive_check, .. } = &mut open.phase
                && matches!(alive_check, AliveCheck::Due | AliveCheck::Asked)
            {
                *alive_check = AliveCheck::Answered;
            }
            Handled::Done
        }
        (_, Ok(Payload::DiagnosticMessage(message))) => {
            if !tx.has_room_for(DIAGNOSTIC_ACK) {
                return Handled::Waiting;
            }
            let (tester, target) = (message.source_address, message.target_address);
            let nack = |tx: &mut TxQueue<CAP>, code| {
                let nack = Message::diagnostic_message_nack(
                    open.version,
                    target,
                    tester,
                    code,
                    &[],
                );
                tx.push(&nack).ok();
            };
            if registered != Some(tester) {
                nack(tx, DiagnosticNackCode::InvalidSourceAddress);
                open.phase = Phase::Finalizing {
                    abort: false,
                    by_caller: false,
                };
                open.deadline = close_by;
                Handled::Done
            } else if let Some(ta_type) = address.ta_type_of(target) {
                tx.push(&Message::diagnostic_message_ack(
                    open.version,
                    target,
                    tester,
                    &[],
                ))
                .ok();
                open.named = true;
                Handled::Indication {
                    sa: tester,
                    ta: target,
                    ta_type,
                    copied: copy(message.user_data, buf),
                    length: message.user_data.len(),
                }
            } else {
                nack(tx, DiagnosticNackCode::UnknownTargetAddress);
                Handled::Done
            }
        }
        _ => Handled::Done,
    };
    rx.consume(consumed);
    handled
}

/// Figure 16's checks of a received header, answering and discarding or closing where
/// one fails.
fn check_header<S, const CAP: usize>(
    open: &mut Open<S>,
    rx: &mut RxBuffer<CAP>,
    tx: &mut TxQueue<CAP>,
    registered: bool,
    limit: usize,
    close_by: Instant,
) -> Result<(), Handled> {
    let header = match rx.header() {
        Ok(header) => header,
        Err(MessageError::Incomplete(_)) => return Err(Handled::Waiting),
        Err(_) => {
            return Err(refuse(open, tx, NackCode::IncorrectPatternFormat, close_by));
        }
    };
    if !matches!(
        header.protocol_version,
        ProtocolVersion::V2012 | ProtocolVersion::V2019
    ) {
        return Err(refuse(open, tx, NackCode::IncorrectPatternFormat, close_by));
    }
    let Some(length_ok) = length_rule(header.payload_type) else {
        return Err(discard(open, tx, rx, &header, NackCode::UnknownPayloadType));
    };
    let fits = |limit: usize| {
        usize::try_from(header.payload_length)
            .is_ok_and(|length| length <= limit.saturating_sub(Header::SIZE))
    };
    if !fits(CAP) {
        return Err(discard(open, tx, rx, &header, NackCode::MessageTooLarge));
    }
    if !registered && !fits(limit) {
        return Err(discard(open, tx, rx, &header, NackCode::OutOfMemory));
    }
    if !length_ok(header.payload_length) {
        return Err(refuse(open, tx, NackCode::InvalidPayloadLength, close_by));
    }
    Ok(())
}

/// The payload lengths ISO 13400-2:2019 allows each payload type a tester sends on a
/// `TCP_DATA` socket, or `None` for a type the entity does not take there (Figure 16).
fn length_rule(payload_type: PayloadType) -> Option<fn(u32) -> bool> {
    match payload_type {
        PayloadType::RoutingActivationRequest => Some(|length| matches!(length, 7 | 11)),
        PayloadType::AliveCheckResponse => Some(|length| length == 2),
        PayloadType::DiagnosticMessage
        | PayloadType::DiagnosticMessagePositiveAcknowledge
        | PayloadType::DiagnosticMessageNegativeAcknowledge => Some(|length| length >= 5),
        PayloadType::NegativeAcknowledge => Some(|length| length == 1),
        _ => None,
    }
}

/// Generic header NACK `code`, then the socket is closed.
fn refuse<S, const CAP: usize>(
    open: &mut Open<S>,
    tx: &mut TxQueue<CAP>,
    code: NackCode,
    close_by: Instant,
) -> Handled {
    if !tx.has_room_for(HEADER_NACK) {
        return Handled::Waiting;
    }
    tx.push(&header_nack(open.version, code)).ok();
    open.phase = Phase::Finalizing {
        abort: false,
        by_caller: false,
    };
    open.deadline = close_by;
    Handled::Done
}

/// Generic header NACK `code`, then the frame's payload is read and dropped.
fn discard<S, const CAP: usize>(
    open: &mut Open<S>,
    tx: &mut TxQueue<CAP>,
    rx: &mut RxBuffer<CAP>,
    header: &Header,
    code: NackCode,
) -> Handled {
    if !tx.has_room_for(HEADER_NACK) {
        return Handled::Waiting;
    }
    tx.push(&header_nack(open.version, code)).ok();
    rx.skip_frame(header);
    Handled::Done
}

fn header_nack(version: ProtocolVersion, code: NackCode) -> Message<'static> {
    Message {
        header: Header::new(version, PayloadType::NegativeAcknowledge, 1),
        payload: Payload::DoIPNack(code),
    }
}

fn copy(data: &[u8], buf: &mut [u8]) -> usize {
    let copied = data.len().min(buf.len());
    if let (Some(to), Some(from)) = (buf.get_mut(..copied), data.get(..copied)) {
        to.copy_from_slice(from);
    }
    copied
}
