//! A compile-time test: this crate's `DoIpTransport` composes with `uds_services`' driver.
//!
//! An `impl` block proves the signatures match `UdsTransport`. It does not prove the
//! type is usable at the places the driver uses it, and that is what this file is for.
//!
//! The test passes by compiling. The checks are the type annotations and the bounds;
//! the functions below are never called. What the transport does at run time is
//! `transport.rs`'s and `end_to_end.rs`'s.

use simple_doip::LogicalAddress;
use simple_doip::TaType;
use simple_doip::service::{
    ConnectionId, DiagnosticEntity, EntityEvent, Refusal, Timestamp,
};
use uds_on_ip::DoIpTransport;
use uds_services::{AfterSend, Ai, TransportEvent, UdsTransport};

/// An entity that is neither `Debug` nor `Send`, and never does anything.
struct OpaqueEntity(core::marker::PhantomData<*const ()>);

#[allow(
    clippy::unused_async_trait_impl,
    reason = "never polled; the bodies exist to satisfy the trait"
)]
impl DiagnosticEntity for OpaqueEntity {
    type Error = ();
    const CONNECTIONS: usize = 1;
    const MAX_PDU: usize = usize::MAX;
    fn request(
        &mut self,
        _sa: LogicalAddress,
        _ta: LogicalAddress,
        _ta_type: TaType,
        _pdu: &[u8],
    ) -> Result<(), Refusal> {
        Ok(())
    }
    fn now(&self) -> Timestamp {
        Timestamp(0)
    }
    async fn next_event<'b>(
        &mut self,
        _buf: &'b mut [u8],
        _deadline: Option<Timestamp>,
    ) -> Result<EntityEvent<'b>, ()> {
        Ok(EntityEvent::Deadline)
    }
    async fn close(&mut self, _connection: ConnectionId) -> Result<(), ()> {
        Ok(())
    }
}

type Transport = DoIpTransport<OpaqueEntity, 1>;
type Error = uds_on_ip::DoIpTransportError<()>;

/// The driver bounds its transport on nothing but the trait.
///
/// `uds_services::Server<A: ServiceSet, T: UdsTransport, const PEERS: usize>`
/// and its client both take the transport by this bound alone, so satisfying it
/// is the whole of what composition requires. Instantiating a `Server` here
/// would need a `ServiceSet` out of the `uds_server!` macro, with handlers and
/// `uds_protocol` message types: scaffolding that tests that macro, not this crate.
fn the_driver_bound_is_satisfied() {
    const fn accepts<T: UdsTransport>() {}
    accepts::<Transport>();
}

/// An entity that is not `Debug` and not `Send` still gives a usable transport.
///
/// The trait's only associated bound is `Error: Debug`, which is this crate's
/// [`uds_on_ip::DoIpTransportError`] over the entity's error. A bound that leaked from the
/// entity itself to the `impl` would show up here.
fn an_opaque_entity_still_satisfies_the_trait() {
    const fn accepts<T: UdsTransport + core::fmt::Debug>() {}
    accepts::<Transport>();
}

/// A request can be answered while the bytes that provoked it are still borrowed.
///
/// `TransportEvent<'b>` borrows the buffer passed to `next_event`, not the
/// transport, so `&mut self` is released when that future completes and
/// `t_data_req` can be called with `data` still live. An event borrowing
/// `&mut self` would make this function fail to compile. `uds_services` proves
/// the same against its own `Loopback`; this proves it against the implementation
/// that carries real traffic.
#[expect(dead_code, reason = "compile-time test: compiled, never called")]
async fn a_request_can_be_answered_while_its_bytes_are_live(
    transport: &mut Transport,
    buffer: &mut [u8],
) -> Result<(), Error> {
    let event = transport.next_event(buffer, None).await?;

    if let TransportEvent::DataInd { ai, data } = event {
        transport.t_data_req(ai, data, AfterSend::Continue).await?;
    }
    Ok(())
}

/// The same property for a truncated message: the case a driver hits whenever it is
/// serving a request and offers only its small concurrent buffer.
///
/// ISO 14229-1:2020 8.7.6 has the server occupied, and the stack answers that
/// request `busyRepeatRequest` (0x21), as its Figure 5's optional busy check does for a
/// different client's request. Composing it means transmitting while the fragment is
/// still borrowed, so the case expected under load is the one that most needs the
/// borrow to be the buffer's.
#[expect(dead_code, reason = "compile-time test: compiled, never called")]
async fn a_truncated_request_can_also_be_answered(
    transport: &mut Transport,
    buffer: &mut [u8],
) -> Result<(), Error> {
    if let TransportEvent::DataTooLong { ai, data, declared } =
        transport.next_event(buffer, None).await?
    {
        let _ = declared;
        transport.t_data_req(ai, data, AfterSend::Continue).await?;
    }
    Ok(())
}

/// The addressing type crossing the seam is `uds_session`'s, reached through
/// `uds_services`' re-export, not a second `Ai` this crate declares.
#[expect(dead_code, reason = "compile-time test: compiled, never called")]
fn the_addressing_is_one_type_across_the_seam(ai: Ai) -> uds_session::Ai {
    ai
}

#[test]
fn the_composition_compiles() {
    // Only the bounds are called; the rest of the test is that this file compiles.
    the_driver_bound_is_satisfied();
    an_opaque_entity_still_satisfies_the_trait();
}
