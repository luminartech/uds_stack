//! The seam below this crate.
//!
//! ``UDSSVC_ARCH_0029`` — this crate calls out to a transport, so this crate declares
//! what it calls. The trait is a transport's whole obligation to the stack: carry bytes
//! both ways, say what the peer will accept, say what timing it dictates, and tell the
//! time. It carries no notion of a service, a data identifier or a negative response
//! code, and nothing in it is `DoIP`-shaped, which is the test of whether it is the
//! right seam. It serves both roles.

pub use uds_session::{Address, Ai, Mtype, Reloads, SResult, TaType, Timestamp};

/// What the transport reports, or that the driver's deadline passed first.
///
/// **The lifetime is the driver's buffer, never the transport.** A message arrives as the
/// subslice of the supplied buffer that it occupies, so the borrow ends when the buffer's
/// does — which is what lets the driver call [`UdsTransport::t_data_req`] while the
/// request bytes that provoked the response are still live. Reporting a length instead
/// would put that tie in prose: the driver would have to re-slice its own buffer and
/// defend against a length larger than what it lent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportEvent<'b> {
    /// A complete inbound message.
    DataInd {
        /// Addressing, with the peer's `S_AI[SA]` — the only way to tell functional
        /// responses apart.
        ai: Ai,
        /// The message, in the buffer the driver supplied.
        data: &'b [u8],
    },
    /// A message longer than the supplied buffer, truncated to what fit.
    ///
    /// A variant rather than a flag on [`Self::DataInd`], because destructuring
    /// `DataInd { ai, data, .. }` is idiomatic and would silently discard a flag,
    /// leaving a fragment decoded as a message. Reached when the driver is serving a
    /// request and offers only the small concurrent buffer, where by clause 8.7.6 the
    /// server is occupied and owes `busyRepeatRequest` (0x21) — a decision needing only
    /// the service identifier and the addressing.
    DataTooLong {
        /// Addressing.
        ai: Ai,
        /// What fit.
        data: &'b [u8],
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
        /// The record, in the buffer the driver supplied.
        data: &'b [u8],
    },
    /// The connection to `peer` went away.
    ///
    /// ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a close **part of the normal
    /// `DiagnosticSessionControl` and `ECUReset` flows**: the server initiates one after
    /// sending every positive `ECUReset` response, and after the positive
    /// `DiagnosticSessionControl` response sent with [`AfterSend::ServerLeaves`], before
    /// executing the service. So a close is not a failure and must not arrive as `Err`.
    /// A transport with no connections never emits this, exactly as one that never
    /// truncates never emits [`Self::DataTooLong`].
    Closed {
        /// The peer whose connection closed: for a server, the client whose requests
        /// it carried. Only an exchange with this peer ends; a transport carrying
        /// several peers' connections reports each close as its own peer's, and one
        /// that cannot name the peer of a connection reports no close for it, because
        /// nothing is waiting on a connection whose peer never spoke.
        peer: Address,
        /// Whether the close was one the standard prescribes.
        ///
        /// **Informational: re-establishing the connection is never this crate's.**
        /// ISO 13400-2:2019 REQ 8.DoIP-144 puts the routing activation request on the
        /// *client* entity, so a server is reconnected *to* and never reconnects, and a
        /// client's own reconnection is its transport's business below this seam
        /// (``UDSSVC_ARCH_0002``). Nothing here asks a transport to reconnect, and no
        /// method to do so exists.
        expected: bool,
    },
    /// The deadline the driver supplied passed before anything arrived.
    ///
    /// **Not itself an overrun.** The driver ticks the session layer and acts on what
    /// that reports; `uds_session::ServerOutput::ResponseOverrun` is the output that
    /// says a response-pending is due.
    Deadline,
}

/// What follows a message handed to [`UdsTransport::t_data_req`].
///
/// The server says it; a transport acts on it. Whether a session change leaves the
/// software the server runs is a property of the server implementation — an application
/// jumping to its bootloader for `10 02`, a bootloader restarting the application for
/// `10 01` (ISO 14229-1:2020 10.2.2.2 Table 25) — and not of the response's octets, so a
/// transport cannot derive it. ISO 14229-5:2022 REQ 7.9 has a `DoIP` server close the TCP
/// connection after the `DiagnosticSessionControl` positive response "if the TCP connection
/// is disconnected due to a session change", so a `DoIP` transport closes on
/// [`Self::ServerLeaves`] and not otherwise.
///
/// **`ECUReset` is never flagged.** REQ 7.11 closes the connection after *every* `ECUReset`
/// positive response, with no condition — including the resets that restart nothing — so
/// a `DoIP` transport keys that close on the `0x51` itself. Flagging resets here would make
/// an unconditional close depend on a fact it does not depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AfterSend {
    /// The server carries on in the software it is running.
    Continue,
    /// The server leaves the software it is running once this message is sent: the
    /// final positive response to a `DiagnosticSessionControl` whose session
    /// [`DiagnosticSessionControl::leaves_running_software`](crate::DiagnosticSessionControl::leaves_running_software).
    ServerLeaves,
}

/// A transport's obligation to the stack.
///
/// ``UDSSVC_ARCH_0030`` for the asynchrony — the trait is `async` and the runtime is
/// the caller's. ``UDSSVC_ARCH_0041`` for the time: a transport that can report an
/// inbound event *or* a timer expiry already measures time, so asking it for the clock
/// adds no capability and avoids two implementors holding two timebases.
///
/// Two obligations the signatures cannot state, both of which the driver relies on:
///
/// - **[`Self::next_event`] is cancel-safe.** The driver races it against the handler
///   and drops the losing future, often unpolled. A message partly read when the future
///   is dropped must not be lost: it is delivered by a later call.
/// - **Every accepted [`Self::t_data_req`] is followed by exactly one
///   [`TransportEvent::DataConf`] for the same [`Ai`]**, including a failed one when the
///   connection closes before the transmission completed. That a transport reports a
///   `T_Data.conf` for every `T_Data.req` is an assumption of use: an association whose
///   confirmation never arrives has no server exit (``UDSS_LLR_0060``), and the driver
///   waits on it.
pub trait UdsTransport {
    /// What this transport's failures are. Never interpreted by this crate.
    ///
    /// `Debug` is the one bound, because it surfaces from [`crate::Server::step`] and
    /// every client method: a caller that receives one and cannot render it has an error
    /// it can only discard. Not interpreting a transport's failures and not being able to
    /// print them are different commitments, and this crate makes only the first. On
    /// `no_std` there is no wider trait to ask for.
    type Error: core::fmt::Debug;

    /// The largest `A_PDU` this transport can carry, where its protocol caps it.
    ///
    /// Participates in the const fold at [`crate::uds_server`]'s expansion site. `DoIP`
    /// has no small fixed cap and leaves the default, which is then ignored.
    const MAX_PDU: usize = usize::MAX;

    /// `T_Data.req` — hand a `T_PDU` to the transport.
    ///
    /// An `Ok` is an acceptance, and obliges exactly one [`TransportEvent::DataConf`]
    /// for `ai` later — failed, where the connection closed first. See the trait's doc.
    ///
    /// # Arguments
    ///
    /// * `ai` - the message's addressing; see [`Ai`].
    /// * `data` - the message, service identifier first.
    /// * `after` - what follows the message; see [`AfterSend`]. A transport without
    ///   connections has nothing to do on [`AfterSend::ServerLeaves`] and ignores it.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the addressing cannot be carried or the link fails.
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        after: AfterSend,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;

    /// Fill `buffer` with the next inbound message, or return
    /// [`TransportEvent::Deadline`] when `deadline` passes first.
    ///
    /// The event borrows `buffer`, not the transport, so `&mut self` is released when the
    /// returned future completes and the driver may transmit while the request is live.
    /// A message is reported as the subslice it occupies; there is no length to overstate.
    ///
    /// `deadline` is `uds_session::Server::next_deadline`'s value passed through
    /// untouched, and the transport is the side that compares it with the clock. The
    /// clock wraps (``UDSS_LLR_0019``), so the comparison is
    /// [`now.has_reached(deadline)`](Timestamp::has_reached) and the time left to wait
    /// is [`now.until(deadline)`](Timestamp::until); never `now >= deadline`, whose
    /// derived order is wrong either side of the wrap and either spins or misses the
    /// deadline by 49.7 days. The deadline may already have been reached when this is
    /// called, and `until` is then zero. `None` means no timer is running: wait for an
    /// event alone.
    ///
    /// A driver serving a request offers only its small concurrent buffer, so
    /// [`TransportEvent::DataTooLong`] is the normal outcome there rather than a fault.
    ///
    /// **Cancel-safe.** The driver drops this future, often unpolled, whenever the
    /// handler it races wins. Dropping it must lose nothing: a message partly read is
    /// kept and delivered by a later call, and a future that consumed input on creation
    /// rather than on its first poll would lose that input.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] where the link fails.
    fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> impl core::future::Future<Output = Result<TransportEvent<'b>, Self::Error>>;

    /// The largest response the peer will accept, where it advertised one.
    ///
    /// ISO 13400-2:2019 Table 11 makes *Max. data size* optional, so `None` is
    /// conformant and leaves the response buffer as the only bound. MDS is defined for
    /// *requests*, so a server asking what it may send is asking about the client.
    ///
    /// There is deliberately no `inbound_max`. This entity's own MDS is the in-flight
    /// buffer's length, which [`crate::uds_server`] derives — so a transport that has to
    /// advertise it needs the crate to *state* the number, not to be asked for it.
    /// Stating it is a seam addition to make when a binding needs it.
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
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the loopback fixture never awaits — it exists to prove the borrow shape \
              of the seam, not to do I/O"
)]
mod tests {
    use super::{AfterSend, TransportEvent, UdsTransport};
    use uds_session::{Address, Ai, Mtype, Reloads, SResult, TaType, Timestamp};

    fn ai() -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E80),
            ta: Address(0x0E00),
            ta_type: TaType::Physical,
        }
    }

    struct Loopback;

    impl UdsTransport for Loopback {
        type Error = ();

        async fn t_data_req(
            &mut self,
            _ai: Ai,
            _data: &[u8],
            _after: AfterSend,
        ) -> Result<(), ()> {
            Ok(())
        }

        async fn next_event<'b>(
            &mut self,
            buffer: &'b mut [u8],
            _deadline: Option<Timestamp>,
        ) -> Result<TransportEvent<'b>, ()> {
            const MESSAGE: [u8; 3] = [0x22, 0xF1, 0x90];
            let n = buffer.len().min(MESSAGE.len());
            let (Some(data), Some(src)) = (buffer.get_mut(..n), MESSAGE.get(..n)) else {
                return Ok(TransportEvent::Deadline);
            };
            data.copy_from_slice(src);
            Ok(TransportEvent::DataInd { ai: ai(), data })
        }

        fn outbound_max(&self) -> Option<usize> {
            None
        }

        fn channel_timing(&self) -> Reloads {
            Reloads {
                default_reload: 2_000,
                enhanced_reload: 5_000,
            }
        }

        fn now(&self) -> Timestamp {
            Timestamp(0)
        }
    }

    /// ``UDSSVC_ARCH_0029`` as amended — the event borrows the *buffer*, so the driver
    /// answers a request while its bytes are still live. This is the shape `Server::step`
    /// needs and the one a borrow of the transport would forbid: an event tied to
    /// `&mut self` keeps the transport mutably borrowed for as long as the request lives,
    /// and `t_data_req` could never be called to answer it.
    ///
    /// This is a compile-time assertion wearing a test's clothes. It fails by not
    /// building.
    #[test]
    fn a_request_can_be_answered_while_its_bytes_are_live() {
        let mut buffer = [0_u8; 8];
        let mut transport = Loopback;
        let sent = core::pin::pin!(async {
            let ev = transport.next_event(&mut buffer, None).await?;
            let TransportEvent::DataInd { ai, data } = ev else {
                return Err(());
            };
            transport.t_data_req(ai, data, AfterSend::Continue).await?;
            Ok::<usize, ()>(data.len())
        });
        assert_eq!(poll_once(sent), Some(Ok(3)));
    }

    /// A truncated message is a distinct variant rather than a flag on `DataInd`.
    /// Destructuring `DataInd { ai, data, .. }` would silently discard a flag and
    /// decode a fragment as a message; a swallowed variant only drops a message the
    /// client is already required to repeat. One fails safe, the other dangerous.
    #[test]
    fn truncation_is_a_variant_an_ordinary_arm_cannot_absorb() {
        let whole = TransportEvent::DataInd {
            ai: ai(),
            data: &[0x22, 0xF1, 0x90],
        };
        let part = TransportEvent::DataTooLong {
            ai: ai(),
            data: &[0x22, 0xF1, 0x90],
            declared: Some(90),
        };
        assert_ne!(whole, part);
        assert!(matches!(whole, TransportEvent::DataInd { .. }));
        assert!(!matches!(part, TransportEvent::DataInd { .. }));
    }

    /// A `DataConf` carries no payload, so it does not constrain the buffer's lifetime.
    #[test]
    fn a_confirmation_carries_no_payload() {
        let c: TransportEvent<'static> = TransportEvent::DataConf {
            ai: ai(),
            result: SResult::Ok,
        };
        assert!(matches!(c, TransportEvent::DataConf { .. }));
    }

    fn poll_once<F: core::future::Future>(
        mut f: core::pin::Pin<&mut F>,
    ) -> Option<F::Output> {
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        match f.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(v) => Some(v),
            core::task::Poll::Pending => None,
        }
    }
}
