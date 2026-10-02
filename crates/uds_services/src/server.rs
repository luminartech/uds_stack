//! The run loop.
//!
//! ``UDSSVC_ARCH_0040`` — this crate owns the loop. It supplies `uds_session` with
//! timestamps and drains its outputs, calls a transport to send and receive bytes, and
//! dispatches inbound requests to the application's handlers. No other crate in the stack
//! contains a driver, and a consuming application writes none.
//!
//! ISO 14229-2 defines a service interface between the session layer and its user and
//! names that user as the ISO 14229-1 layer. A third party sitting between them is a
//! component no standard describes, and it is the component that went a full design cycle
//! with no owner.

use crate::ResponseSink;
use crate::select::{Either, select2};
use crate::services::{Responded, ServiceSet};
use crate::storage::{Buffers, Storage};
use crate::transport::{TransportEvent, UdsTransport};
pub use uds_session::ServerParams;
use uds_session::{
    Ai, Association, SResult, Server as SessionServer, ServerOutput, ServerRx, ServerTx,
    Solicitation,
};

/// The UDS server: an application's services, its storage, a session layer and a
/// transport.
///
/// Named `Server` although `uds_session::Server` exists, because a consuming application
/// never imports `uds_session` — only a binding author does, and a binding author can
/// alias. The trait an application implements is [`ServiceSet`], so `Server` means one
/// thing here.
///
/// `PEERS` is a parameter rather than a derived constant because the session layer's type
/// is `uds_session::Server<PEERS>`, and an associated const of a generic cannot be a const
/// generic argument. An application does not write it: [`crate::uds_server`]'s
/// `peers = N` supplies it and the macro emits the alias, so the count appears once.
#[derive(Debug)]
pub struct Server<A: ServiceSet, T: UdsTransport, const PEERS: usize> {
    services: A,
    store: A::Store,
    session: SessionServer<PEERS>,
    transport: T,
}

impl<A: ServiceSet, T: UdsTransport, const PEERS: usize> Server<A, T, PEERS> {
    /// Assemble a server.
    ///
    /// A `const fn`, which is load-bearing: the storage is inline and can be several
    /// kilobytes, so a runtime constructor would build a stack temporary before the move
    /// and a small-stack target could not hold it. `static SERVER: EcuServer =
    /// EcuServer::new(..)` constructs in place, which is what
    /// [`Storage::EMPTY`] being an associated const buys.
    ///
    /// The driver builds its own session, so an application never names `uds_session`.
    pub const fn new(services: A, transport: T, params: ServerParams) -> Self {
        Self {
            services,
            store: <A::Store as Storage>::EMPTY,
            session: SessionServer::new([Association::EMPTY; PEERS], params),
            transport,
        }
    }

    /// The application's services, for whatever it needs them for between events.
    pub fn services(&mut self) -> &mut A {
        &mut self.services
    }

    /// Handle one transport event.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed. A negative response is not an
    /// error: it is a response, written into the sink.
    pub async fn step(&mut self) -> Result<(), T::Error> {
        let Buffers {
            in_flight,
            concurrent,
            response,
        } = self.store.split();

        let deadline = self.session.next_deadline();

        let ev = self
            .transport
            .next_event(&mut in_flight[..], deadline)
            .await?;
        let TransportEvent::DataInd {
            ai,
            data: request_bytes,
        } = ev
        else {
            // Everything that is not a request is dropped here, and each case is owed
            // something this stub does not yet do: a DataTooLong at this point exceeds
            // this entity's own MDS and owes busyRepeatRequest or 0x13; a DataConf must
            // reach uds_session's t_data_conf; a Deadline outside a handler must reach
            // tick(); Periodic and Closed have no handler at all. Listed in this task's
            // report as elided behaviour, not implemented here.
            return Ok(());
        };

        // Sampled here, not earlier: `outbound_max` and `now` take `&self`, but a live
        // `next_event` future refuses even shared access, so both have to be read after
        // that await returns. It is also what makes a peer limit learned during the
        // exchange that delivered this request apply to its response. `now()` is called
        // once per input handed to uds_session, three times in this body, matching its
        // "a timestamp accompanies every input".
        let now = self.transport.now();
        let outbound_max = self.transport.outbound_max();
        let mut indication = None;
        let mut reaction = self.session.t_data_ind(
            now,
            ai,
            request_bytes,
            SResult::Ok,
            ServerRx::Request { session: None },
        );
        // The `Indicate` is taken out of the drain before `finish()`: a `ServerOutput<'d>`
        // outlives the reaction that yielded it, which is what lets dispatch run with
        // `&mut session` free. Required, or the 0x78 window does not exist.
        for out in reaction.outputs() {
            if let ServerOutput::Indicate {
                ai,
                data,
                result: SResult::Ok,
            } = out
            {
                indication = Some((ai, data));
            }
        }
        let _ = reaction.finish();
        let Some((ai, request)) = indication else {
            return Ok(());
        };
        // The byte is kept as received because a negative response echoes it
        // (ISO 14229-1:2020 Table 21, SIDRQ), and `UdsServiceType::to_request_sid` maps
        // every value it does not model back to 0x7F.
        //
        // Elided: an empty request carries no service identifier, so clause 8.7's first
        // check cannot run on it and it owes incorrectMessageLengthOrInvalidFormat
        // (0x13). Dropping it is this stub's behaviour, not the intended one.
        let Some(sid) = request.first().copied() else {
            return Ok(());
        };

        let mut sink = ResponseSink::new(response, outbound_max);
        let pending_deadline = self.session.next_deadline();

        // Two scopes, because `pin!` binds to the enclosing block: `handler` is scoped so
        // it drops before `sink` is read, and `waiting` is scoped per iteration so it
        // drops before the 0x78 path needs `&mut transport`.
        let outcome = {
            let mut handler = core::pin::pin!(self.services.dispatch(request, &mut sink));
            let mut deadline = pending_deadline;
            // A loop over `handler.as_mut()`, not a one-shot: a plain
            // `select2(handler, waiting)` drops the handler the moment the deadline wins,
            // which is the opposite of what a response-pending is for.
            loop {
                let event = {
                    let waiting = core::pin::pin!(
                        self.transport.next_event(&mut concurrent[..], deadline)
                    );
                    select2(handler.as_mut(), waiting).await
                };
                match event {
                    Either::Left(done) => break done,
                    Either::Right(Ok(TransportEvent::Deadline)) => {
                        answer_overrun(&mut self.session, &mut self.transport, ai, sid)
                            .await?;
                        deadline = self.session.next_deadline();
                    }
                    Either::Right(Ok(TransportEvent::Closed { expected })) => {
                        // The exchange is over either way: a server does not reconnect
                        // (ISO 13400-2 REQ 8.DoIP-144 puts routing activation on the
                        // client), so there is nothing to do but stop.
                        //
                        // A close reaching *this* arm is never REQ 7.9's or 7.11's. Those
                        // follow a positive response, and nothing positive has been sent
                        // yet -- the only thing this loop hands to `t_data_req` is
                        // `answer_overrun`'s response-pending, whose first octet is 0x7F.
                        // So `expected` needs no examination here; a mid-handler close is
                        // a dropped link or a tester leaving early.
                        //
                        // Elided: which is why reporting it as `Suppressed` is wrong. That
                        // says clause 8.7 required no response, where the truth is that
                        // none could be sent. Distinguishing them is the pipeline's, and
                        // the pipeline is `todo!()`.
                        let _ = expected;
                        break Ok(Responded::Suppressed);
                    }
                    Either::Right(Ok(_concurrent)) => {
                        // Elided: a concurrent message that is not one of clause
                        // 8.7.6's two exceptions (ServiceSet::is_concurrent_exception)
                        // owes busyRepeatRequest (0x21), not silence — acting on the
                        // classification is open question 1. A DataTooLong here is
                        // occupancy of the concurrent buffer and owes 0x21 too, and
                        // must never lower the advertised MDS. Neither is done today;
                        // this arm only re-arms the deadline.
                        deadline = self.session.next_deadline();
                    }
                    Either::Right(Err(e)) => return Err(e),
                }
            }
        };

        if !matches!(outcome, Ok(Responded::Yes)) {
            return Ok(());
        }

        let now = self.transport.now();
        let reaction = self.session.s_data_req(
            now,
            ai,
            sink.written_bytes(),
            ServerTx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: None,
            },
        );
        transmit_all(reaction, &mut self.transport).await
    }

    /// Run until the transport fails.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returned.
    pub async fn run(&mut self) -> Result<core::convert::Infallible, T::Error> {
        loop {
            self.step().await?;
        }
    }
}

/// Drain a reaction, sending every `Transmit` it yields.
///
/// Inside the drain, not after it: a reaction may yield several and all of them must
/// reach the transport. The reaction borrows the session and `t_data_req` borrows the
/// transport, which is why they arrive as two arguments rather than as one `&mut self` —
/// disjoint fields are disjoint borrows only while nothing has merged them.
async fn transmit_all<T: UdsTransport, const PEERS: usize>(
    mut reaction: uds_session::ServerReaction<'_, '_, PEERS>,
    transport: &mut T,
) -> Result<(), T::Error> {
    for out in reaction.outputs() {
        if let ServerOutput::Transmit { ai, data } = out {
            transport.t_data_req(ai, data).await?;
        }
    }
    let _ = reaction.finish();
    Ok(())
}

/// Answer `requestCorrectlyReceivedResponsePending` (0x78) if one has come due.
///
/// ``UDSSVC_ARCH_0031``. The deadline passing is not itself the overrun: the session
/// layer is ticked and `UDSS_LLR_0117`'s `ResponseOverrun` is the output that says a 0x78
/// is owed. A deadline that passed for any other reason produces no message.
async fn answer_overrun<T: UdsTransport, const PEERS: usize>(
    session: &mut SessionServer<PEERS>,
    transport: &mut T,
    ai: Ai,
    sid: u8,
) -> Result<(), T::Error> {
    let now = transport.now();
    let mut tick = session.tick(now);
    let overran = tick
        .outputs()
        .any(|out| matches!(out, ServerOutput::ResponseOverrun { .. }));
    let _ = tick.finish();
    if !overran {
        return Ok(());
    }
    let pending = [0x7F_u8, sid, 0x78];
    let reaction = session.s_data_req(now, ai, &pending, ServerTx::ResponsePending);
    transmit_all(reaction, transport).await
}

#[cfg(test)]
mod tests {
    use uds_session::ServerParams;

    /// The driver holds a `uds_session::Server<PEERS>`, and an associated const of a
    /// generic parameter cannot be a const generic argument — the same
    /// `generic_const_exprs` wall the buffers hit. So `PEERS` is a parameter of this
    /// type, supplied by `uds_server!`'s `peers = N` through the alias it emits.
    ///
    /// The real construction is `tests/composition.rs`; here only the params shape is
    /// asserted, because a concrete `ServiceSet` does not exist yet in this crate.
    #[test]
    fn the_session_parameters_have_no_spacing() {
        const PARAMS: ServerParams = ServerParams {
            s3_server: 5_000,
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        };
        assert_eq!(PARAMS.p2_server_max, 50);
    }
}
