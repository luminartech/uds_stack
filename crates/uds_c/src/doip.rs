//! The `DoIP` entity and transport the firmware's sockets carry, and the poll that drives
//! a server over them.
//!
//! The entity accepts one connection at a time, plus the reserve socket
//! `simple_doip` keeps for a second routing activation, and discovers on the one UDP
//! socket. It is built in place, without discovery, so a server holding it is a
//! `static`; [`start_discovery`] starts discovery once the interface has an address.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use simple_doip::entity::{Discovery, EntityAddress, FixedIdentity};
use simple_doip::service::EntityConfig;
use uds_on_ip::DoIpTransport;
use uds_services::{Reloads, Stopped};

use crate::nal::{Acceptor, Datagrams};
use crate::sys;

/// The entity over the firmware's sockets: one connection, messages of up to
/// `MAX_MESSAGE` bytes with their header, and `TESTERS` testers allowed to activate
/// routing.
pub type Entity<const MAX_MESSAGE: usize, const TESTERS: usize> =
    simple_doip::entity::Entity<
        'static,
        Acceptor,
        1,
        MAX_MESSAGE,
        TESTERS,
        Option<Discovery<Datagrams, FixedIdentity>>,
    >;

/// The transport over [`Entity`]: one slot per entity socket, the reserve included.
pub type Transport<const MAX_MESSAGE: usize, const TESTERS: usize> =
    DoIpTransport<Entity<MAX_MESSAGE, TESTERS>, 2>;

/// The transport for the entity at `address`, admitting the testers `config` names, with
/// discovery not yet started. A `const fn`, so a server holding it is built as a
/// `static`.
#[must_use]
pub const fn transport<const MAX_MESSAGE: usize, const TESTERS: usize>(
    address: EntityAddress,
    config: EntityConfig<TESTERS>,
    reloads: Reloads,
) -> Transport<MAX_MESSAGE, TESTERS> {
    DoIpTransport::new(
        simple_doip::entity::Entity::new_discovery_later(&Acceptor, address, config),
        reloads,
    )
}

/// Starts discovery on the firmware's UDP socket as `identity`, its announcement jitter
/// seeded from `uds_now_ms`. Called once, when the interface has a valid address and
/// before the server is first polled.
pub fn start_discovery<const MAX_MESSAGE: usize, const TESTERS: usize>(
    transport: &mut Transport<MAX_MESSAGE, TESTERS>,
    identity: FixedIdentity,
) {
    transport
        .entity_mut()
        .start_discovery(Datagrams, identity, sys::uds_now_ms());
}

/// Why [`poll_run`] did not poll a running server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError<E> {
    /// No run future is armed: the server was never started, or it stopped earlier.
    NotStarted,
    /// The server stopped on a transport error, and its run future was dropped.
    Stopped(E),
}

/// Polls the server's run future once, with a waker that does nothing: the firmware
/// polls on every pass of its loop.
///
/// A server that stops on a confirmed reset is at its power-up state; its finished
/// future is dropped, then `rearm` builds the next one, so the server keeps serving. A
/// server that stops on a transport error is left unarmed.
///
/// `run` lives where the firmware keeps it across polls, typically a `static`, since a
/// future dropped mid-step loses its request.
///
/// # Errors
///
/// [`RunError::NotStarted`] for an empty `run`; [`RunError::Stopped`] for the transport
/// error the server stopped on.
pub fn poll_run<F, E>(
    mut run: Pin<&mut Option<F>>,
    rearm: impl FnOnce() -> F,
) -> Result<(), RunError<E>>
where
    F: Future<Output = Result<Stopped, E>>,
{
    let Some(future) = run.as_mut().as_pin_mut() else {
        return Err(RunError::NotStarted);
    };
    match future.poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Pending => Ok(()),
        Poll::Ready(Ok(_)) => {
            // The finished future may still borrow the server: dropped before `rearm`.
            run.set(None);
            run.set(Some(rearm()));
            Ok(())
        }
        Poll::Ready(Err(error)) => {
            run.set(None);
            Err(RunError::Stopped(error))
        }
    }
}
