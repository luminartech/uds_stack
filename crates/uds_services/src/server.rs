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
use uds_session::{
    Association, SResult, Server as SessionServer, ServerOutput, ServerParams, ServerRx,
    ServerTx, Solicitation,
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
/// generic argument. **[`crate::uds_server`] emits no type alias**: an application writes
/// `Server<Ecu, T, N>` out by hand, as `tests/composition.rs` does. So the peer count is
/// stated twice — once as `channels = N` in the assembly, where nothing reads it, and
/// once here, where it actually sizes the association array — and nothing diagnoses a
/// disagreement between them.
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
    /// [`Storage::EMPTY`](crate::Storage::EMPTY) being an associated const buys.
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
    /// Four properties of the body below are load-bearing and none is obvious:
    ///
    /// 1. **Every transport query is sampled while no future holding `&mut transport`
    ///    exists.** `outbound_max` and `now` take `&self`, but a live `next_event`
    ///    future refuses even shared access, so both are read after that await returns
    ///    — which is also what makes a peer limit learned during the exchange that
    ///    delivered this request apply to its response. `now()` is called once per input
    ///    handed to `uds_session`, three times in this body, which matches its "a
    ///    timestamp accompanies every input".
    /// 2. **The `Indicate` comes out of the drain before `finish()`.** A
    ///    `ServerOutput<'d>` outlives the reaction that yielded it, which is what lets
    ///    dispatch run with `&mut session` free — required, or the 0x78 window does not
    ///    exist.
    /// 3. **The select is a loop over `handler.as_mut()`, not a one-shot.** A plain
    ///    `select2(handler, waiting)` drops the handler the moment the deadline wins,
    ///    which is the opposite of what a response-pending is for.
    /// 4. **Two scopes, because `pin!` binds to the enclosing block.** `handler` is
    ///    scoped so it drops before `sink` is read; `waiting` is scoped per iteration so
    ///    it drops before the 0x78 path needs `&mut transport`.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed. A negative response is not an
    /// error: it is a response, written into the sink.
    #[allow(
        clippy::too_many_lines,
        reason = "the four load-bearing properties above are properties of this body's \
                  borrow order; splitting it into helpers would pass &mut self.session \
                  and &mut self.transport across a call boundary, which is exactly what \
                  the scoping this function relies on exists to avoid"
    )]
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
        let sid = request.first().copied().unwrap_or(0);

        let mut sink = ResponseSink::new(response, outbound_max);
        let pending_deadline = self.session.next_deadline();

        let outcome = {
            let mut handler = core::pin::pin!(self.services.dispatch(request, &mut sink));
            let mut deadline = pending_deadline;
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
                        // The deadline passing is not itself the overrun. Tick the
                        // session and act on what it reports: UDSS_LLR_0117's
                        // ResponseOverrun is the output that says a 0x78 is due.
                        let now = self.transport.now();
                        let mut overran = false;
                        let mut tick = self.session.tick(now);
                        for out in tick.outputs() {
                            if let ServerOutput::ResponseOverrun { .. } = out {
                                overran = true;
                            }
                        }
                        let _ = tick.finish();

                        if overran {
                            let pending = [0x7F_u8, sid, 0x78];
                            let mut r = self.session.s_data_req(
                                now,
                                ai,
                                &pending,
                                ServerTx::ResponsePending,
                            );
                            for out in r.outputs() {
                                if let ServerOutput::Transmit { ai, data } = out {
                                    self.transport.t_data_req(ai, data).await?;
                                }
                            }
                            let _sent = r.finish().is_ok();
                        }
                        deadline = self.session.next_deadline();
                    }
                    Either::Right(Ok(TransportEvent::Closed { expected })) => {
                        // REQ 7.9 / 7.11 make an expected close part of the normal
                        // DiagnosticSessionControl and ECUReset flows, so it ends the
                        // exchange without failing it. An unexpected close is the same
                        // shape here; what differs is what the caller does next.
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
        let mut r = self.session.s_data_req(
            now,
            ai,
            sink.written_bytes(),
            ServerTx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: None,
            },
        );
        // Sent inside the drain: every Transmit must reach the transport, not just the
        // last. The reaction borrows &mut session and t_data_req borrows &mut transport,
        // which are disjoint fields.
        for out in r.outputs() {
            if let ServerOutput::Transmit { ai, data } = out {
                self.transport.t_data_req(ai, data).await?;
            }
        }
        let _ = r.finish();
        Ok(())
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

#[cfg(test)]
mod tests {
    use uds_session::ServerParams;

    /// The driver holds a `uds_session::Server<PEERS>`, and an associated const of a
    /// generic parameter cannot be a const generic argument — the same
    /// `generic_const_exprs` wall the buffers hit. So `PEERS` is a parameter of this
    /// type, and an application writes it out: the macro emits no alias, which is why
    /// the peer count is stated twice with nothing checking that the two agree.
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
