//! This crate's `DoIpTransport` composes with `uds_services`' driver.
//!
//! `uds_services` published `UdsTransport` with one implementation behind it —
//! a `Loopback` fixture in its own test module — and said plainly that ours
//! would be the first real one. An `impl` block proves the signatures match. It
//! does not prove the type is usable at the places the driver uses it, and that
//! is what this file is for.
//!
//! Nothing here runs. Every body in `DoIpTransport` is still a `todo!()`, so
//! the assertions are the type annotations and the bounds; the functions exist
//! to be compiled, never to be polled.

#![allow(dead_code, reason = "type-checked, never run")]

use uds_on_ip::DoIpTransport;
use uds_services::{Ai, TransportEvent, UdsTransport};

/// The driver bounds its transport on nothing but the trait.
///
/// `uds_services::Server<A: ServiceSet, T: UdsTransport, const PEERS: usize>`
/// and its client both take the transport by this bound alone, so satisfying it
/// is the whole of what composition requires. Instantiating a `Server` here
/// would additionally need a `ServiceSet` out of the `uds_server!` macro, with
/// handlers and `uds_protocol` message types — scaffolding that would test that
/// macro rather than this crate.
fn the_driver_bound_is_satisfied() {
    const fn accepts<T: UdsTransport>() {}
    accepts::<DoIpTransport<()>>();
}

/// A socket that is not `Debug` and not `Send` still gives a usable transport.
///
/// The trait's only associated bound is `Error: Debug`, which is this crate's
/// [`uds_on_ip::Error`], not the socket's. A bound that leaked from `S` to the
/// `impl` would show up here.
fn an_opaque_socket_still_satisfies_the_trait() {
    struct OpaqueSocket(core::marker::PhantomData<*const ()>);
    const fn accepts<T: UdsTransport>() {}
    accepts::<DoIpTransport<OpaqueSocket>>();
}

/// The property the seam's whole shape exists for: a request can be answered
/// while the bytes that provoked it are still borrowed.
///
/// `TransportEvent<'b>` borrows the buffer passed to `next_event`, not the
/// transport, so `&mut self` is released when that future completes and
/// `t_data_req` can be called with `data` still live. An event borrowing
/// `&mut self` would make this function fail to compile — which is exactly what
/// the earlier mirrored event type did, and why `uds_services` reshaped it.
///
/// `uds_services` proves this against its own `Loopback`. This proves it
/// against the implementation that will carry real traffic.
///
/// # Verified non-vacuous, from both sides
///
/// A test that merely compiles can be true for the wrong reason, so both were
/// checked by injection. Using `data` *after* the `t_data_req` call still
/// compiles, so it is genuinely live across the transmit rather than dropped
/// before it. Re-receiving into the same `buffer` while `data` is live fails
/// with `E0499`, so the borrow checker is engaged here rather than waved
/// through.
///
/// What is *not* injected is the failure this guards: an event borrowing
/// `&mut self` would have to come from editing `uds_services`, which is not
/// this repository's to do.
async fn a_request_can_be_answered_while_its_bytes_are_live(
    transport: &mut DoIpTransport<()>,
    buffer: &mut [u8],
) -> Result<(), uds_on_ip::Error> {
    let event = transport.next_event(buffer, None).await?;

    // `TransportEvent` is `#[non_exhaustive]`, so this cannot be an exhaustive
    // match from outside `uds_services` — the borrow is what is under test, not
    // the case coverage.
    if let TransportEvent::DataInd { ai, data } = event {
        transport.t_data_req(ai, data).await?;
    }
    Ok(())
}

/// The same property for a truncated message, which is the case a driver hits
/// whenever it is serving a request and offers only its small concurrent
/// buffer.
///
/// ISO 14229-1 8.7.6 owes that request `busyRepeatRequest` (0x21), and
/// composing one means transmitting while the fragment is still borrowed — so
/// the case that is *expected* to occur under load is the case that most needs
/// the borrow to be the buffer's.
async fn a_truncated_request_can_also_be_answered(
    transport: &mut DoIpTransport<()>,
    buffer: &mut [u8],
) -> Result<(), uds_on_ip::Error> {
    if let TransportEvent::DataTooLong { ai, data, declared } =
        transport.next_event(buffer, None).await?
    {
        let _ = declared;
        transport.t_data_req(ai, data).await?;
    }
    Ok(())
}

/// The addressing type crossing the seam is `uds_session`'s, reached through
/// `uds_services`' re-export — not a second `Ai` this crate declares.
fn the_addressing_is_one_type_across_the_seam(ai: Ai) -> uds_session::Ai {
    ai
}

#[test]
fn the_composition_compiles() {
    // The assertion is the build itself.
    the_driver_bound_is_satisfied();
    an_opaque_socket_still_satisfies_the_trait();
}
