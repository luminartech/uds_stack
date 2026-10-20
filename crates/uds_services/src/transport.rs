//! The seam below this crate.
//!
//! ``UDSSVC_ARCH_0029``: this crate calls out to a transport, so this crate declares
//! what it calls. The trait is a transport's whole obligation to the stack, for both
//! roles: carry bytes both ways, say what the peer will accept, say what timing it
//! dictates, and tell the time. It carries no notion of a service, a data identifier or
//! a negative response code, and nothing in it is `DoIP`-shaped.

pub use uds_session::{Address, Ai, Mtype, Reloads, SResult, TaType, Timestamp};

/// What the transport reports, or that the driver's deadline passed first.
///
/// A message arrives as the subslice of the supplied buffer that it occupies, so the
/// borrow is the buffer's, not the transport's, and the driver calls
/// [`UdsTransport::t_data_req`] while the request bytes that provoked the response are
/// still live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransportEvent<'b> {
    /// A complete inbound message.
    DataInd {
        /// Addressing, with the peer's `S_AI[SA]`, which tells functional responses
        /// apart.
        ai: Ai,
        /// The message, in the buffer the driver supplied.
        data: &'b [u8],
    },
    /// A message longer than the supplied buffer, truncated to what fit.
    ///
    /// A variant rather than a flag on [`Self::DataInd`], so that destructuring
    /// `DataInd { ai, data, .. }` cannot decode a fragment as a message. The normal
    /// outcome while the driver is serving a request and offers only the small
    /// concurrent buffer: by clause 8.7.6 the server is occupied and owes
    /// `busyRepeatRequest` (0x21), decided from the service identifier and the
    /// addressing alone.
    DataTooLong {
        /// Addressing.
        ai: Ai,
        /// What fit.
        data: &'b [u8],
        /// How long the message was, where the transport knows (`DoIP` reads it from the
        /// generic header); `None` where it cannot know without reading the whole
        /// message.
        declared: Option<usize>,
    },
    /// The outcome of a requested transmission.
    ///
    /// Raised from the acknowledgment rather than from the socket write returning,
    /// because the acknowledgment is what starts `tP_Client`
    /// (ISO 14229-2:2021 REQ 5.9).
    DataConf {
        /// The addressing of the transmission being confirmed.
        ai: Ai,
        /// The outcome.
        result: SResult,
    },
    /// A periodic response, ISO 14229-5:2022 REQ 7.16's `0x8004` payload type.
    ///
    /// Never a [`Self::DataInd`]: ISO 14229-5:2022 REQ 7.20 requires a periodic response
    /// not to reset `tS3_Server`, and `DataInd` is what feeds the session layer and
    /// resets it. The driver does nothing with it: `ReadDataByPeriodicIdentifier` (0x2A)
    /// has no `uds_protocol` message type, so no server can implement it and no client
    /// can decode the result.
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
    /// Not a failure, so never an `Err`: ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a
    /// close part of the normal `DiagnosticSessionControl` and `ECUReset` flows, the
    /// server closing after every positive `ECUReset` response and after the positive
    /// `DiagnosticSessionControl` response sent with [`AfterSend::ServerLeaves`]. A
    /// transport with no connections never emits this.
    ///
    /// Reported once per close, as an edge: the next [`UdsTransport::next_event`] waits
    /// for an event or the deadline as usual, and a request handed to the transport
    /// meanwhile is still confirmed, failed, as the trait requires.
    Closed {
        /// The peer whose connection closed: for a server, the client whose requests
        /// it carried; for a client, the server it sent them to. Only an exchange with
        /// this peer ends, a client's functional request counting as one with every
        /// peer. A transport carrying several peers' connections reports each close as
        /// its own peer's, and one that cannot name the peer of a connection reports no
        /// close for it.
        peer: Address,
        /// Whether the close was one the standard prescribes, which ends the server's
        /// session: for a server, one it owed its client; for a client, one following the
        /// server's positive `DiagnosticSessionControl` or `ECUReset` response
        /// (ISO 14229-5:2022 REQ 7.9, REQ 7.11). Any other close, a failure or a connection
        /// given up, ends a connection and not a session.
        ///
        /// Informational: ISO 14229-5:2022 REQ 7.8 and REQ 7.10 have the client open a
        /// new connection, so a server never reconnects, and a client's reconnection is
        /// its transport's, below this seam (``UDSSVC_ARCH_0002``).
        expected: bool,
    },
    /// The deadline the driver supplied passed before anything arrived.
    ///
    /// Not itself an overrun: the driver ticks the session layer, and
    /// `uds_session::ServerOutput::ResponseOverrun` is the output that says a
    /// response-pending is due.
    Deadline,
}

/// What follows a message handed to [`UdsTransport::t_data_req`].
///
/// The server says it; a transport acts on it. Whether a session change leaves the
/// software the server runs (ISO 14229-1:2020 10.2.2.2 Table 25) is the server's to know,
/// not derivable from the response's octets. ISO 14229-5:2022 REQ 7.9 has a `DoIP` server
/// close the TCP connection after a `DiagnosticSessionControl` positive response "if the
/// TCP connection is disconnected due to a session change", so a `DoIP` transport closes
/// on [`Self::ServerLeaves`] and not otherwise. `ECUReset` is never flagged: REQ 7.11
/// closes after *every* `ECUReset` positive response, so a `DoIP` transport keys that
/// close on the `0x51` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
/// The trait is `async` and the runtime is the caller's (``UDSSVC_ARCH_0030``). The
/// transport tells the time (``UDSSVC_ARCH_0041``): one that can report an inbound event
/// *or* a timer expiry already measures it.
///
/// Four obligations the signatures cannot state, which the drivers rely on:
///
/// - **[`Self::next_event`] is cancel-safe.** The driver races it against the handler
///   and drops the losing future, often unpolled. A message partly read when the future
///   is dropped must not be lost: it is delivered by a later call.
/// - **Every accepted [`Self::t_data_req`] is followed by exactly one
///   [`TransportEvent::DataConf`] for the same [`Ai`]**, including a failed one when the
///   connection closes before the transmission completed. An association whose
///   confirmation never arrives has no server exit (``UDSS_LLR_0060``), and the driver
///   waits on it.
/// - **A [`Self::t_data_req`] future dropped before it completes has accepted its
///   request or done nothing.** An accepted request is owed its
///   [`TransportEvent::DataConf`] as any other is; one not accepted is never sent.
/// - **That [`TransportEvent::DataConf`] comes before any message answering the
///   request.** The session layer opens a request's response window at its confirmation,
///   so a client takes nothing earlier as its answer.
pub trait UdsTransport {
    /// What this transport's failures are. Never interpreted by this crate.
    ///
    /// `Debug` is the one bound: the error surfaces from [`crate::Server::step`] and
    /// every client method, and a caller has to be able to render it.
    type Error: core::fmt::Debug;

    /// The largest `A_PDU` this transport can carry, where its protocol caps it.
    ///
    /// Joins the const fold at [`crate::uds_server`]'s expansion site. The default, for
    /// a transport with no cap, is then ignored; a `DoIP` transport takes its entity's
    /// message buffer.
    const MAX_PDU: usize = usize::MAX;

    /// `T_Data.req`: hand a `T_PDU` to the transport.
    ///
    /// An `Ok` is an acceptance, and obliges exactly one [`TransportEvent::DataConf`]
    /// for `ai` later, failed where the connection closed first. Dropped before it
    /// completes, the future has accepted the request or done nothing.
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
    /// The event borrows `buffer`, not the transport, so the driver may transmit while
    /// the request is live. A message is reported as the subslice it occupies.
    ///
    /// The clock wraps (``UDSS_LLR_0019``), so the transport compares `deadline` with it
    /// through [`now.has_reached(deadline)`](Timestamp::has_reached) and waits
    /// [`now.until(deadline)`](Timestamp::until), never by comparing the raw counts. The
    /// deadline may already have been reached when this is called, and `until` is then
    /// zero. `None` means no timer is running: wait for an event alone.
    ///
    /// A driver serving a request offers only its small concurrent buffer, so
    /// [`TransportEvent::DataTooLong`] is the normal outcome there rather than a fault.
    ///
    /// **Must be cancel-safe.** The driver drops this future, often unpolled, whenever
    /// the handler it races wins: a message partly read is kept and delivered by a later
    /// call, and the future consumes no input before its first poll.
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
    /// There is no `inbound_max`: a request longer than the server's in-flight buffer
    /// still reaches the server, as [`TransportEvent::DataTooLong`], so the pipeline
    /// answers it in ISO 14229-1's order, an unsupported service `0x11` before a wrong
    /// length `0x13`. A transport refuses only what it cannot hold itself.
    fn outbound_max(&self) -> Option<usize>;

    /// The `tP_Client` reload pair this transport dictates (`DoIP` has no `T_DataSOM.ind`,
    /// hence `tP6` rather than `tP2`). `tP3` spacing is not part of it; the client
    /// configures that in [`crate::ClientTiming`]. ISO 14229-2:2021 Table 4 sets the
    /// minimum per transport, and its footnotes d and e leave the maximum to the client.
    fn channel_timing(&self) -> Reloads;

    /// Monotonic milliseconds, 32-bit and wrapping.
    fn now(&self) -> Timestamp;

    /// The server is restarting: [`crate::Server::step`] is about to return
    /// [`Stopped::Reset`](crate::Stopped::Reset), its reset's response sent or suppressed.
    ///
    /// A transport ends here what the restart ends and no response closed, as a `DoIP`
    /// transport closes its testers' connections after a suppressed `ECUReset`
    /// (ISO 14229-5:2022 REQ 7.11). Does nothing unless a transport says otherwise.
    fn restarting(&mut self) -> impl core::future::Future<Output = ()> {
        core::future::ready(())
    }
}

/// A transport a client sends over: a [`UdsTransport`] whose connection the client can
/// end.
///
/// Only a client ends a session. The one close a server makes, the prescribed close of
/// ISO 14229-5:2022 REQ 7.9 and REQ 7.11, its transport makes itself on
/// [`AfterSend::ServerLeaves`] or a positive `ECUReset` response.
pub trait ClientTransport: UdsTransport {
    /// Ends the connection to the server gracefully, if there is one.
    ///
    /// A request accepted and not yet confirmed is still confirmed, failed, by a later
    /// [`UdsTransport::next_event`]. The close is reported by no
    /// [`TransportEvent::Closed`]: the caller made it. A transport with nothing to close
    /// returns `Ok`. The next [`UdsTransport::t_data_req`] opens a connection again
    /// where the transport has connections to open.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where closing fails; the connection is closed either way.
    fn close(&mut self) -> impl core::future::Future<Output = Result<(), Self::Error>>;
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

    /// ``UDSSVC_ARCH_0029``: the event borrows the *buffer*, so the driver answers a
    /// request while its bytes are still live. An event tied to `&mut self` would keep
    /// the transport borrowed for as long as the request lives, and `t_data_req` could
    /// never be called to answer it. This fails by not building.
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

    /// A truncated message is a distinct variant rather than a flag on `DataInd`, so
    /// destructuring `DataInd { ai, data, .. }` cannot decode a fragment as a message.
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
