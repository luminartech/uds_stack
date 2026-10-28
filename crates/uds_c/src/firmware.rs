//! The server and its run future in one `static`, polled from one execution context: the
//! way firmware drives the server.
//!
//! The firmware still names the run future's type, which only `type_alias_impl_trait`
//! can do for a `static`; the rest, the cells, arming and the poll, is here.

#![allow(
    unsafe_code,
    reason = "a static's cells, reached through the unsafe calls' single-context contract"
)]

use core::cell::{Cell, UnsafeCell};
use core::fmt;
use core::future::Future;
use core::pin::Pin;

use uds_services::{Server, ServiceSet, StepError, Stopped, UdsTransport};

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
    /// Whether a terminal transport error stopped the server, which is then never armed
    /// again.
    failed: Cell<bool>,
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
            failed: Cell::new(false),
        }
    }
}

impl<S: TableServer, F> Firmware<S, F> {
    /// Arms the server at start-up: `start` prepares it, then `run` builds the future
    /// [`Self::poll`] drives. Returns `false`, calling neither, where it is armed already
    /// or a terminal transport error stopped it ([`RunError::Stopped`]): that server is
    /// never stepped again, and serves again only once the firmware restarts. The
    /// [`Validated`] it takes is what [`TableApp::validate`] returned for the server's
    /// own application.
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
        if slot.is_some() || self.failed.get() {
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
    /// positive responses the stack transmitted since the last pass, as
    /// `uds_tcp_transmitted` reports it, each through every layer of the server's policy,
    /// as `include/uds_c.h` orders them, then one poll of the run future, with a waker
    /// that does nothing. A run future that stops on a confirmed reset leaves the server
    /// at its power-up state, and `run` builds the next one, so the server keeps
    /// serving. One that stops on a transport error between requests, which the server
    /// recovers from, leaves the server where it was, a response awaiting its
    /// confirmation included, and `run` builds the next one over the same server too. One
    /// that stops on a terminal transport error, mid-request, leaves the server unarmed
    /// for good.
    ///
    /// # Errors
    ///
    /// [`RunError::NotStarted`] before [`Self::arm`] or after a terminal transport error;
    /// [`RunError::Interrupted`] for a recoverable transport error, the server serving
    /// on; [`RunError::Stopped`] for the terminal transport error the server stopped on.
    ///
    /// # Safety
    ///
    /// The contract in [`Firmware`]'s `# Safety`.
    pub unsafe fn poll<E>(
        &'static self,
        run: impl FnOnce(&'static mut S) -> F,
    ) -> Result<(), RunError<E>>
    where
        F: Future<Output = Result<Stopped, StepError<E>>>,
    {
        execute_deferred(S::LAYERS);
        // SAFETY: the contract makes this the only reference to the slot; it is behind
        // `&'static self`, so the future in it is never moved or freed, and only
        // dropped in place.
        let slot = unsafe { Pin::new_unchecked(&mut *self.run.get()) };
        let polled = doip::poll_run(slot, || {
            // SAFETY: `poll_run` drops the finished future before it calls this, and by
            // the contract on `run` that future held the only reference to the server.
            run(unsafe { &mut *self.server.get() })
        });
        if matches!(polled, Err(RunError::Stopped(_))) {
            self.failed.set(true);
        }
        polled
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

    /// How a run future ends.
    #[derive(Debug, Clone, Copy)]
    enum End {
        /// On a confirmed reset.
        Reset,
        /// On a transport error, recoverable or terminal.
        Fails(StepError<()>),
    }

    const RESET: End = End::Reset;
    const RECOVERABLE: End = End::Fails(StepError::Recoverable(()));
    const TERMINAL: End = End::Fails(StepError::Terminal(()));

    /// The server's run future: pending `pending` polls, then ended as `end` says.
    #[derive(Debug)]
    struct Run {
        server: &'static mut Server,
        pending: u32,
        end: End,
    }

    impl Future for Run {
        type Output = Result<Stopped, StepError<()>>;

        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            self.server.polls += 1;
            if self.pending > 0 {
                self.pending -= 1;
                return Poll::Pending;
            }
            Poll::Ready(match self.end {
                End::Reset => Ok(Stopped::Reset(ResetType::HardReset)),
                End::Fails(error) => Err(error),
            })
        }
    }

    /// Builds a run future, keeping the server reference in it alone.
    fn run(pending: u32, end: End) -> impl FnOnce(&'static mut Server) -> Run {
        move |server| {
            server.runs += 1;
            Run {
                server,
                pending,
                end,
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
        let polled = unsafe { FIRMWARE.poll(run(0, RESET)) };
        assert!(matches!(polled, Err(RunError::NotStarted)));
    }

    #[test]
    fn arming_twice_starts_the_server_once() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(5, RESET)));
            assert!(!FIRMWARE.arm(validated(), start, run(5, RESET)));
            assert!(FIRMWARE.poll(run(5, RESET)).is_ok());
        }
        // SAFETY: as above; this read invalidates the armed run future's reference
        // for writes, which is sound only because the future is never polled again.
        let server = unsafe { &*FIRMWARE.server.get() };
        assert_eq!((server.starts, server.runs, server.polls), (1, 1, 1));
    }

    #[test]
    fn a_reset_or_a_recoverable_error_builds_the_run_again() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(1, RESET)));
            assert!(FIRMWARE.poll(run(0, RECOVERABLE)).is_ok());
            // The first run stops on a reset, and the second is built, failing
            // recoverably when polled; the third is built over the same server.
            assert!(FIRMWARE.poll(run(0, RECOVERABLE)).is_ok());
            assert!(matches!(
                FIRMWARE.poll(run(1, RESET)),
                Err(RunError::Interrupted(()))
            ));
            assert!(!FIRMWARE.arm(validated(), start, run(0, RESET)));
            assert!(FIRMWARE.poll(run(0, RESET)).is_ok());
        }
        // SAFETY: as above; the armed run future is never polled again.
        let server = unsafe { &*FIRMWARE.server.get() };
        assert_eq!((server.starts, server.runs, server.polls), (1, 3, 4));
    }

    #[test]
    fn a_terminal_error_unarms_the_run_for_good() {
        static FIRMWARE: Firmware<Server, Run> = Firmware::new(server());
        // SAFETY: as above.
        unsafe {
            assert!(FIRMWARE.arm(validated(), start, run(0, TERMINAL)));
            assert!(matches!(
                FIRMWARE.poll(run(0, RESET)),
                Err(RunError::Stopped(()))
            ));
            assert!(matches!(
                FIRMWARE.poll(run(0, RESET)),
                Err(RunError::NotStarted)
            ));
            assert!(!FIRMWARE.arm(validated(), start, run(0, RESET)));
            assert!(matches!(
                FIRMWARE.poll(run(0, RESET)),
                Err(RunError::NotStarted)
            ));
        }
        assert_eq!(counters(&FIRMWARE), (1, 1, 1));
    }
}
