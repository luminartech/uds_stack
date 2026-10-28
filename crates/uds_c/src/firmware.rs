//! The server and its run future in one `static`, polled from one execution context: the
//! way firmware drives the server.
//!
//! The firmware still names the run future's type, which only `type_alias_impl_trait`
//! can do for a `static`; the rest, the cells, arming and the poll, is here.

#![allow(
    unsafe_code,
    reason = "a static's cells, reached through the unsafe calls' single-context contract"
)]

use core::cell::UnsafeCell;
use core::fmt;
use core::future::Future;
use core::pin::Pin;

use uds_services::{Server, ServiceSet, Stopped, UdsTransport};

use crate::app::execute_deferred;
use crate::doip::{self, RunError};
use crate::{Layers, Policy, TableApp, Validated};

mod sealed {
    pub trait Sealed {}
}

/// A server over a [`TableApp`]: what a [`Firmware`] arms and polls. Implemented for
/// every `uds_services::Server` whose application is a `TableApp`, and sealed.
pub trait TableServer: sealed::Sealed {
    /// The server's application, whose [`Validated`] proof [`Firmware::arm`] takes.
    type App;
    /// The layers the application serves, through which [`Firmware::poll`] makes the
    /// resets and session changes its responses confirmed.
    const LAYERS: &'static Layers;
    /// Whether the assembly declines what the tables do not serve: was given
    /// `serves = App::serves`. [`Firmware::arm`] fails to compile without it.
    const DECLINES: bool;
}

impl<A, T, const PEERS: usize> sealed::Sealed for Server<A, T, PEERS>
where
    A: ServiceSet,
    T: UdsTransport,
{
}

impl<A, T, P, const PEERS: usize, const RECORD: usize, const LEVELS: usize> TableServer
    for Server<A, T, PEERS>
where
    A: ServiceSet<App = TableApp<P, RECORD, LEVELS>>,
    T: UdsTransport,
    P: Policy,
{
    type App = TableApp<P, RECORD, LEVELS>;
    const LAYERS: &'static Layers = P::LAYERS;
    const DECLINES: bool = A::DECLINES;
}

/// A server and the run future [`Self::poll`] drives, in one `static`.
///
/// Placing the whole of it, its server included, where the firmware wants it, with
/// `#[link_section]` say, keeps the server off a small stack: [`Self::new`] is a
/// `const fn`, so it is initialized data rather than built at run time.
///
/// # Safety
///
/// [`Self::arm`] and [`Self::poll`] share one contract, which is what makes a `static`
/// of it sound:
///
/// - Every call is made from one execution context, the one that polls the server, and
///   none is made while another is under way: from a handler the server calls, say, or
///   from an interrupt.
/// - The server was built in that context, or is a `static`'s constant initializer, so
///   an `S` that is not `Send` never reaches it from another thread.
/// - Each `run` closure moves the server reference it is given into the future it
///   returns and nowhere else, so that reference ends with the future.
pub struct Firmware<S, F> {
    /// Ahead of `server`, so a `Firmware` dropped drops the future before the server it
    /// borrows.
    run: UnsafeCell<Option<F>>,
    server: UnsafeCell<S>,
}

// SAFETY: every access to the cells is through `arm` or `poll`, whose contract confines
// them to one execution context and never re-enters one, so the cells are never reached
// from two threads, nor twice at once. Neither `S` nor `F` need be `Send`: the contract
// has the server built in that context or constant, and `F` is built there.
unsafe impl<S, F> Sync for Firmware<S, F> {}

impl<S, F> fmt::Debug for Firmware<S, F> {
    /// Opaque: reading the cells here would race the context that polls them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Firmware").finish_non_exhaustive()
    }
}

impl<S, F> Firmware<S, F> {
    /// `server`, not yet armed.
    #[must_use]
    pub const fn new(server: S) -> Self {
        Self {
            run: UnsafeCell::new(None),
            server: UnsafeCell::new(server),
        }
    }
}

impl<S: TableServer, F> Firmware<S, F> {
    /// Arms the server where it has no run future, at start-up or after a transport
    /// error stopped it: `start` prepares it, then `run` builds the future
    /// [`Self::poll`] drives. Returns `false`, calling neither, where it is armed
    /// already. The [`Validated`] it takes is what [`TableApp::validate`] returned for
    /// the server's own application.
    ///
    /// Fails to compile for a server whose assembly was not given
    /// `serves = App::serves`, without which a service the tables do not serve is
    /// refused only after the checks ahead of its handler, not `serviceNotSupported`
    /// (0x11) before them.
    ///
    /// # Safety
    ///
    /// The contract in [`Firmware`]'s `# Safety`.
    pub unsafe fn arm(
        &'static self,
        _: Validated<S::App>,
        start: impl FnOnce(&mut S),
        run: impl FnOnce(&'static mut S) -> F,
    ) -> bool {
        const {
            assert!(
                S::DECLINES,
                "the server's `uds_server!` needs `serves = App::serves`"
            );
        };
        // SAFETY: the contract makes this call the only one under way, so nothing else
        // reaches the slot.
        let slot = unsafe { &mut *self.run.get() };
        if slot.is_some() {
            return false;
        }
        // SAFETY: no run future, so, by the contract on `run`, no reference to the
        // server survives, and this call is the only one under way.
        let server = unsafe { &mut *self.server.get() };
        start(server);
        *slot = Some(run(server));
        true
    }

    /// One pass of the firmware's loop: first the resets and session changes whose
    /// positive responses `uds_tcp_write` took since the last pass, each through every
    /// layer of the server's policy, as `include/uds_c.h` orders them, then one poll of
    /// the run future, with a waker that does nothing. A run future that stops on a
    /// confirmed reset leaves the server at its power-up state, and `run` builds the
    /// next one, so the server keeps serving; one that stops on a transport error leaves
    /// the server unarmed.
    ///
    /// # Errors
    ///
    /// [`RunError::NotStarted`] before [`Self::arm`] or after a transport error;
    /// [`RunError::Stopped`] for the transport error the server stopped on.
    ///
    /// # Safety
    ///
    /// The contract in [`Firmware`]'s `# Safety`.
    pub unsafe fn poll<E>(
        &'static self,
        run: impl FnOnce(&'static mut S) -> F,
    ) -> Result<(), RunError<E>>
    where
        F: Future<Output = Result<Stopped, E>>,
    {
        execute_deferred(S::LAYERS);
        // SAFETY: the contract makes this the only reference to the slot; it is behind
        // `&'static self`, so the future in it is never moved or freed, and only
        // dropped in place.
        let slot = unsafe { Pin::new_unchecked(&mut *self.run.get()) };
        doip::poll_run(slot, || {
            // SAFETY: `poll_run` drops the finished future before it calls this, and by
            // the contract on `run` that future held the only reference to the server.
            run(unsafe { &mut *self.server.get() })
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    reason = "test harness: a few polls of a counted run future, over tables with nothing \
              to refuse"
)]
mod tests {
    use core::task::{Context, Poll};

    use uds_protocol::ResetType;
    use uds_services::{DiagnosticSessionType, P2StarServerMax, SessionTiming, Sessions};

    use super::*;
    use crate::{Policy, TableApp};

    static NO_LAYERS: [&crate::UdsLayer; 0] = [];
    // SAFETY: no layer, so no handler.
    static LAYERS: Layers = unsafe { Layers::new(&NO_LAYERS) };

    /// A policy over [`LAYERS`].
    struct Empty;

    impl Policy for Empty {
        const LAYERS: &'static Layers = &LAYERS;
        const SESSIONS: Sessions = Sessions::ALL;

        fn timing(_session: DiagnosticSessionType) -> SessionTiming {
            SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max: P2StarServerMax::from_tens_of_millis(500),
            }
        }
    }

    fn validated() -> Validated<TableApp<Empty>> {
        TableApp::<Empty>::validate().expect("no tables, so nothing to refuse")
    }

    /// A server that records its starts, its runs and their polls.
    #[derive(Debug)]
    struct Server {
        starts: u32,
        runs: u32,
        polls: u32,
    }

    impl sealed::Sealed for Server {}

    impl TableServer for Server {
        type App = TableApp<Empty>;
        const LAYERS: &'static Layers = &LAYERS;
        const DECLINES: bool = true;
    }

    /// The server's run future: pending `pending` polls, then stopped on a reset, or on a
    /// transport error where it `fails`.
    #[derive(Debug)]
    struct Run {
        server: &'static mut Server,
        pending: u32,
        fails: bool,
    }

    impl Future for Run {
        type Output = Result<Stopped, ()>;

        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            self.server.polls += 1;
            if self.pending > 0 {
                self.pending -= 1;
                Poll::Pending
            } else if self.fails {
                Poll::Ready(Err(()))
            } else {
                Poll::Ready(Ok(Stopped::Reset(ResetType::HardReset)))
            }
        }
    }

    /// Builds a run future, keeping the server reference in it alone.
    fn run(pending: u32, fails: bool) -> impl FnOnce(&'static mut Server) -> Run {
        move |server| {
            server.runs += 1;
            Run {
                server,
                pending,
                fails,
            }
        }
    }

    fn start(server: &mut Server) {
        server.starts += 1;
    }

    const fn server() -> Server {
        Server {
            starts: 0,
            runs: 0,
            polls: 0,
        }
    }

    /// The server's counters, read where no run future is armed.
    fn counters(firmware: &'static Firmware<Server, Run>) -> (u32, u32, u32) {
        // SAFETY: each test's static is reached from that test's thread alone, and no
        // run future is armed, so nothing else references the server.
        let server = unsafe { &*firmware.server.get() };
        (server.starts, server.runs, server.polls)
    }

    #[test]
    fn a_poll_before_arming_reports_it_not_started() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: each test's static is reached from that test's thread alone, and each
        // `run` keeps the server reference in its future.
        let polled = unsafe { FIRMWARE.poll(run(0, false)) };
        assert!(matches!(polled, Err(RunError::NotStarted)));
    }

    #[test]
    fn arming_twice_starts_the_server_once() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(5, false)));
            assert!(!FIRMWARE.arm(validated(), start, run(5, false)));
            assert!(FIRMWARE.poll(run(5, false)).is_ok());
        }
        // SAFETY: as above; this read invalidates the armed run future's reference
        // for writes, which is sound only because the future is never polled again.
        let server = unsafe { &*FIRMWARE.server.get() };
        assert_eq!((server.starts, server.runs, server.polls), (1, 1, 1));
    }

    #[test]
    fn a_reset_builds_the_run_again_and_a_transport_error_unarms_it() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(1, false)));
            assert!(FIRMWARE.poll(run(0, true)).is_ok());
            // The first run stops on a reset, and the second is built, failing when
            // polled.
            assert!(FIRMWARE.poll(run(0, true)).is_ok());
            assert!(matches!(
                FIRMWARE.poll(run(0, true)),
                Err(RunError::Stopped(()))
            ));
            assert!(matches!(
                FIRMWARE.poll(run(0, true)),
                Err(RunError::NotStarted)
            ));
        }
        assert_eq!(counters(&FIRMWARE), (1, 2, 3));
        // Unarmed by the error, so arming starts the server again.
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(0, false)));
            assert!(FIRMWARE.poll(run(0, false)).is_ok());
        }
    }
}
