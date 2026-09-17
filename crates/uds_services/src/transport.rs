//! The seam below this crate.
//!
//! ``UDSSVC_ARCH_0029`` — this crate calls out to a transport, so this crate declares
//! what it calls. The trait is a transport's whole obligation to the stack: carry bytes
//! both ways, say what the peer will accept, say what timing it dictates, and tell the
//! time. It carries no notion of a service, a data identifier or a negative response
//! code, and nothing in it is `DoIP`-shaped, which is the test of whether it is the
//! right seam. It serves both roles.

use uds_session::{Ai, Reloads, SResult, Timestamp};

/// What the transport reports, or that the driver's deadline passed first.
///
/// **No lifetime.** The payload went into the buffer the driver supplied, so nothing
/// here borrows the transport — which is what lets the driver call
/// [`UdsTransport::t_data_req`] while the request bytes that provoked the response are
/// still live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransportEvent {
    /// A complete inbound message, in the first `len` bytes of the supplied buffer.
    DataInd {
        /// Addressing, with the peer's `S_AI[SA]` — the only way to tell functional
        /// responses apart.
        ai: Ai,
        /// How much of the buffer the message occupies.
        len: usize,
    },
    /// A message longer than the supplied buffer. The first `len` bytes are present.
    ///
    /// A variant rather than a flag on [`Self::DataInd`], because destructuring
    /// `DataInd { ai, len, .. }` is idiomatic and would silently discard a flag,
    /// leaving a fragment decoded as a message. Reached when the driver is serving a
    /// request and offers only the small concurrent buffer, where by clause 8.7.6 the
    /// server is occupied and owes `busyRepeatRequest` (0x21) — a decision needing only
    /// the service identifier and the addressing.
    DataTooLong {
        /// Addressing.
        ai: Ai,
        /// How much fit.
        len: usize,
        /// How long the message actually was, where the transport knows. `DoIP` reads
        /// it from the generic header before the payload, so it is free there; a
        /// transport that cannot know without reading the whole message reports `None`
        /// rather than inventing one.
        declared: Option<usize>,
    },
    /// The outcome of a requested transmission.
    ///
    /// Raised from the acknowledgement rather than from the socket write returning,
    /// because the acknowledgement is what starts `tP_Client`
    /// (ISO 14229-2:2021 REQ 5.9).
    DataConf {
        /// The addressing of the transmission being confirmed.
        ai: Ai,
        /// The outcome.
        result: SResult,
    },
    /// A periodic response — ISO 14229-5:2022 REQ 7.16's `0x8004` payload type.
    ///
    /// **Cannot be a [`Self::DataInd`].** REQ 7.20 requires a periodic response not to
    /// reset `tS3_Server`, and `DataInd` is exactly what feeds the session layer and
    /// resets it — so delivering one as a `DataInd` is a conformance failure, not a
    /// shortcut. `DoIP`-specific in its *payload type* but not in its shape: a CAN
    /// binding implementing periodic responses reports the same three things.
    ///
    /// The driver carries this variant and does nothing with it yet:
    /// `ReadDataByPeriodicIdentifier` (0x2A) has no `uds_protocol` message type, so no
    /// server can implement it and no client can decode the result.
    Periodic {
        /// Addressing, with the responding server's `S_AI[SA]`.
        ai: Ai,
        /// The periodic data identifier.
        pdid: u8,
        /// How much of the supplied buffer the record occupies.
        len: usize,
    },
    /// The connection went away.
    ///
    /// ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a server-initiated close **part of
    /// the normal `DiagnosticSessionControl` and `ECUReset` flows**, so an expected
    /// close is not a failure and must not arrive as `Err`. A transport with no
    /// connections never emits this, exactly as one that never truncates never emits
    /// [`Self::DataTooLong`].
    Closed {
        /// Whether the close was part of a flow the standard prescribes. The driver's
        /// decision is binary — reconnect and repeat routing activation, or fail the
        /// exchange — and the reason behind an unexpected close is not something a
        /// driver can act on differently.
        expected: bool,
    },
    /// The deadline the driver supplied passed before anything arrived.
    ///
    /// **Not itself an overrun.** The driver ticks the session layer and acts on what
    /// that reports; `uds_session::ServerOutput::ResponseOverrun` is the output that
    /// says a response-pending is due.
    Deadline,
}

/// A transport's obligation to the stack.
///
/// ``UDSSVC_ARCH_0030`` for the asynchrony — the trait is `async` and the runtime is
/// the caller's. ``UDSSVC_ARCH_0041`` for the time: a transport that can report an
/// inbound event *or* a timer expiry already measures time, so asking it for the clock
/// adds no capability and avoids two implementors holding two timebases.
pub trait UdsTransport {
    /// What this transport's failures are. Never interpreted by this crate.
    type Error;

    /// The largest `A_PDU` this transport can carry, where its protocol caps it.
    ///
    /// Participates in the const fold at [`crate::uds_server`]'s expansion site. `DoIP`
    /// has no small fixed cap and leaves the default, which is then ignored.
    const MAX_PDU: usize = usize::MAX;

    /// `T_Data.req` — hand a `T_PDU` to the transport.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the addressing cannot be carried or the link fails.
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;

    /// Fill `buffer` with the next inbound message, or return
    /// [`TransportEvent::Deadline`] when `deadline` passes first.
    ///
    /// The buffer is the driver's, so the borrow ends when this returns. `deadline` is
    /// `uds_session::Server::next_deadline`'s value passed through untouched —
    /// `Timestamp` carries `UDSS_LLR_0019`'s modular arithmetic, so neither side of
    /// this seam computes an interval.
    ///
    /// A driver serving a request offers only its small concurrent buffer, so
    /// [`TransportEvent::DataTooLong`] is the normal outcome there rather than a fault.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the link fails.
    fn next_event(
        &mut self,
        buffer: &mut [u8],
        deadline: Option<Timestamp>,
    ) -> impl core::future::Future<Output = Result<TransportEvent, Self::Error>>;

    /// The largest request this entity will accept, where it advertises one.
    ///
    /// ISO 13400-2:2019 Table 11 makes *Max. data size* optional, so `None` is
    /// conformant. Its value is the in-flight buffer's length, which this crate derives.
    ///
    /// **The route that would hand a transport that number is unbuilt.** The trait has
    /// this getter and nothing else: nothing in this crate calls it and nothing supplies
    /// the length, so a binding today has to invent the very value the assembly already
    /// derived. Closing it needs a way for the crate to *state* the length, which is a
    /// seam decision rather than a missing setter.
    fn inbound_max(&self) -> Option<usize>;

    /// The largest response the peer will accept, where it advertised one.
    ///
    /// MDS is defined for *requests*, so a server asking what it may send is asking
    /// about the client.
    fn outbound_max(&self) -> Option<usize>;

    /// The `tP_Client` reload pair this transport dictates.
    ///
    /// The pair alone: a transport dictates it — `DoIP` has no `T_DataSOM.ind`, hence
    /// `tP6` rather than `tP2` — but has no view on `tP3` spacing, which is
    /// ISO 14229-2 clause 9.7 client policy.
    fn channel_timing(&self) -> Reloads;

    /// Monotonic milliseconds, 32-bit and wrapping.
    fn now(&self) -> Timestamp;
}

#[cfg(test)]
mod tests {
    use super::TransportEvent;
    use uds_session::{Address, Ai, Mtype, SResult, TaType};

    fn ai() -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0E00),
            ta_type: TaType::Physical,
        }
    }

    /// ``UDSSVC_ARCH_0029`` as amended — the event carries a length into the caller's
    /// buffer, never a borrow of the transport. A borrowed event keeps the transport
    /// mutably borrowed for as long as the request lives, so the driver could never
    /// call `t_data_req` to answer it.
    ///
    /// **This assertion has a trap.** It fails by `TransportEvent` no longer resolving
    /// without a lifetime argument, and the obvious repair — writing
    /// `TransportEvent<'_>` — makes `'_` infer `'static`, so the bound passes trivially
    /// and the guarantee is gone while this test stays green. Verify it by
    /// reintroducing a borrowing variant and watching it fail, not by watching it pass.
    #[test]
    fn an_event_borrows_nothing() {
        fn assert_static<T: 'static>(_: &T) {}
        assert_static(&TransportEvent::DataInd { ai: ai(), len: 7 });
        assert_static(&TransportEvent::DataTooLong {
            ai: ai(),
            len: 8,
            declared: Some(1_024),
        });
        assert_static(&TransportEvent::DataConf {
            ai: ai(),
            result: SResult::Ok,
        });
        assert_static(&TransportEvent::Periodic {
            ai: ai(),
            pdid: 0x01,
            len: 4,
        });
        assert_static(&TransportEvent::Closed { expected: true });
        assert_static(&TransportEvent::Deadline);
    }

    /// A truncated message is a distinct variant rather than a flag on `DataInd`.
    /// Destructuring `DataInd { ai, len, .. }` would silently discard a flag and
    /// decode a fragment as a message; a swallowed variant only drops a message the
    /// client is already required to repeat. One fails safe, the other dangerous.
    #[test]
    fn truncation_is_a_variant_an_ordinary_arm_cannot_absorb() {
        let whole = TransportEvent::DataInd { ai: ai(), len: 4 };
        let part = TransportEvent::DataTooLong {
            ai: ai(),
            len: 4,
            declared: Some(90),
        };
        assert_ne!(whole, part);
        assert!(matches!(whole, TransportEvent::DataInd { .. }));
        assert!(!matches!(part, TransportEvent::DataInd { .. }));
    }
}
