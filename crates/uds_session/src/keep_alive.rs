//! The client's keep-alive modes, ``UDSS_LLR_0149`` to ``UDSS_LLR_0163``.

use crate::classification::{
    ClientRx, ClientTx, ExpectedResponses, SessionSelection, Solicitation,
};
use crate::time::Timestamp;
use crate::timer::{Reaches, Timer};

pub(crate) use role::{Event, PhysicalSession, Site};

mod role {
    use crate::classification::{ClientRx, ClientTx};
    use crate::time::Timestamp;

    /// What a keep-alive mode keeps and does. Public in a private module, so it can bound
    /// [`super::KeepAliveMode`] while no caller can implement it or name it by path. Its
    /// items stay reachable through that bound, so they are public surface; none of them
    /// delivers an output, and the only mode value a caller holds is one not yet moved
    /// into a client.
    pub trait Role: core::fmt::Debug {
        /// The mode's state in each physical channel's storage (``UDSS_LLR_0151``).
        type Channel: Copy + core::fmt::Debug;

        /// Whether the mode's `TesterPresent` goes out on a functional channel.
        const FUNCTIONAL: bool;

        /// Act on `event`, which happened at `site`.
        fn on(&mut self, now: Timestamp, site: Site<'_, Self::Channel>, event: Event);

        /// ``UDSS_LLR_0156`` — whether the client-wide keep-alive fell due at `now`.
        fn expire(&mut self, now: Timestamp) -> bool;

        /// ``UDSS_LLR_0162`` — whether `channel`'s keep-alive fell due at `now`.
        fn expire_channel(channel: &mut Self::Channel, now: Timestamp) -> bool;

        fn deadline(&self) -> Option<Timestamp>;

        fn channel_deadline(channel: &Self::Channel) -> Option<Timestamp>;
    }

    /// The kind of channel an event happened on, with a physical channel's own state.
    #[derive(Debug)]
    pub enum Site<'a, C> {
        Physical(&'a mut C),
        Functional,
    }

    /// What the client tells its keep-alive mode.
    #[derive(Debug, Clone, Copy)]
    pub enum Event {
        /// A request was handed to the transport (``UDSS_LLR_0160``).
        Sent,
        /// A transmission completed (``UDSS_LLR_0155`` to ``UDSS_LLR_0163``).
        Confirmed { ok: bool, class: ClientTx },
        /// A message was received (``UDSS_LLR_0159``, ``UDSS_LLR_0161``,
        /// ``UDSS_LLR_0163``).
        Received { ok: bool, class: Option<ClientRx> },
        /// The window of a request marked keep-alive expired (``UDSS_LLR_0161``).
        KeepAliveWindowExpired,
        /// The caller released the keep-alive (``UDSS_LLR_0184``).
        Released,
        /// The caller reset the channel (``UDSS_LLR_0180``).
        Reset,
    }

    /// ``UDSS_LLR_0151`` — a physical channel's `tS3_Client`, its reload and its
    /// session fact.
    #[derive(Debug, Clone, Copy)]
    pub struct PhysicalSession {
        pub(crate) reload: u32,
        pub(crate) s3: crate::timer::Timer<crate::timer::Reaches>,
        pub(crate) in_session: bool,
    }

    impl PhysicalSession {
        pub(crate) fn leave(&mut self) {
            self.in_session = false;
            self.s3.stop();
        }

        /// ``UDSS_LLR_0153`` — not in session, the timer stopped.
        pub(crate) const fn new(reload: u32) -> Self {
            Self {
                reload,
                s3: crate::timer::Timer::STOPPED,
                in_session: false,
            }
        }
    }
}

/// How the client keeps servers alive.
///
/// ``UDSS_LLR_0149`` — one of two modes, fixed when the instance is created, changed by
/// no input. The mode selects which state ``UDSS_LLR_0150`` or ``UDSS_LLR_0151``
/// requires, and which of ``UDSS_LLR_0155`` to ``UDSS_LLR_0163`` and ``UDSS_LLR_0184``
/// act.
///
/// It is a type parameter of [`crate::Client`] rather than a value inside it because
/// ``UDSS_LLR_0149`` settles it at creation and nothing afterwards can move it. Holding it
/// in the type is what lets ``UDSS_LLR_0152`` be satisfied without a check: the methods
/// that supply a `tS3_Client` reload exist only on the mode that gives one a meaning, so
/// none of that requirement's three disagreements can be written.
///
/// The trait is sealed. A mode is not an extension point — the standard names two — and
/// ``UDSS_LLR_0011`` forbids the session layer to deliver an output through a
/// caller-supplied trait implementation, which sealing keeps true of every trait here.
pub trait KeepAliveMode: crate::sealed::Sealed + role::Role {}

/// Functional keep-alive: one `TesterPresent` for the client, functionally addressed.
///
/// ``UDSS_LLR_0150`` — a single `tS3_Client` timer and a single keeping-alive fact for
/// the instance, with the single reload of ``UDSS_LLR_0152``. ISO 14229-2:2021 9.6 Table 8
/// allots one timer here, so this value is fixed in size; it is caller-supplied all the
/// same, because ``UDSS_LLR_0008`` puts every fact the client holds in the caller's
/// storage and a fact with nothing left to size is no exception.
/// Storage is moved into the instance, never duplicated — see [`crate::Association`]. A
/// copy of this is a second `tS3_Client` timer, which ``UDSS_LLR_0150`` gives the client
/// exactly one of.
#[derive(Debug)]
pub struct FunctionalKeepAlive {
    pub(crate) reload: u32,
    s3: Timer<Reaches>,
    keeping_alive: bool,
}

impl FunctionalKeepAlive {
    /// The initial state, with the client-wide `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0153`` — no session is kept alive and the timer is not running.
    /// ``UDSS_LLR_0152`` — `s3_client` must cover the longest path among every server the
    /// functional address reaches. [`crate::Client::set_keep_alive_reload`] sets it again.
    #[must_use]
    pub const fn new(s3_client: u32) -> Self {
        Self {
            reload: s3_client,
            s3: Timer::STOPPED,
            keeping_alive: false,
        }
    }
}

impl crate::sealed::Sealed for FunctionalKeepAlive {}
impl KeepAliveMode for FunctionalKeepAlive {}

impl role::Role for FunctionalKeepAlive {
    type Channel = ();
    const FUNCTIONAL: bool = true;

    fn on(&mut self, now: Timestamp, site: Site<'_, ()>, event: Event) {
        let functional = matches!(site, Site::Functional);
        match event {
            Event::Confirmed { ok: true, class } => {
                let selects = class.session_selection();
                if selects == Some(SessionSelection::NonDefault) && !self.s3.is_running() {
                    self.keeping_alive = true; // UDSS_LLR_0155
                    self.s3.start(now, self.reload);
                }
                if functional && self.keeping_alive {
                    if matches!(class, ClientTx::KeepAlive { .. }) {
                        self.s3.start(now, self.reload); // UDSS_LLR_0157
                    } else if selects == Some(SessionSelection::Default) {
                        self.release(); // UDSS_LLR_0158
                    }
                }
            }
            Event::Released if functional => self.release(), // UDSS_LLR_0184
            _ => {}
        }
    }

    fn expire(&mut self, now: Timestamp) -> bool {
        let due = self.keeping_alive && self.s3.expired(now);
        if due {
            self.s3.stop(); // UDSS_LLR_0156
        }
        due
    }

    fn expire_channel((): &mut (), _: Timestamp) -> bool {
        false
    }

    fn deadline(&self) -> Option<Timestamp> {
        self.s3.deadline()
    }

    fn channel_deadline((): &()) -> Option<Timestamp> {
        None
    }
}

impl FunctionalKeepAlive {
    fn release(&mut self) {
        self.keeping_alive = false;
        self.s3.stop();
    }
}

/// Physical keep-alive: a `TesterPresent` per physical channel, physically addressed.
///
/// ``UDSS_LLR_0151`` — the timer and session fact live in each physical channel's own
/// storage, and ``UDSS_LLR_0152`` gives each physical channel its own reload, supplied at
/// [`crate::Client::open_physical_channel`]. Nothing is client-wide, so this mode
/// carries no value at all.
/// It holds nothing, but it is still moved rather than copied, so that a keep-alive mode
/// reaches [`crate::Client::new`] the same way in both modes — see [`crate::Association`].
#[derive(Debug)]
pub struct PhysicalKeepAlive;

impl crate::sealed::Sealed for PhysicalKeepAlive {}
impl KeepAliveMode for PhysicalKeepAlive {}

impl role::Role for PhysicalKeepAlive {
    type Channel = role::PhysicalSession;
    const FUNCTIONAL: bool = false;

    fn on(&mut self, now: Timestamp, site: Site<'_, PhysicalSession>, event: Event) {
        let Site::Physical(s) = site else {
            return; // UDSS_LLR_0149: a functional channel keeps nothing alive here
        };
        // Where the event completes an exchange, the session it leaves the server in.
        let completed = match event {
            Event::Confirmed { ok: false, .. }
            | Event::Received { ok: false, .. }
            | Event::KeepAliveWindowExpired => Some(None),
            Event::Reset if !s.s3.is_running() => Some(None), // UDSS_LLR_0180
            Event::Confirmed { ok: true, class }
                if class.expected() == ExpectedResponses::None =>
            {
                Some(class.session_selection())
            }
            Event::Received {
                ok: true,
                class:
                    Some(ClientRx::FinalResponse {
                        solicitation: Solicitation::Solicited,
                        session,
                    }),
            } => Some(session),
            Event::Sent => {
                s.s3.stop(); // UDSS_LLR_0160
                None
            }
            Event::Released => {
                s.leave(); // UDSS_LLR_0184
                None
            }
            Event::Confirmed { .. } | Event::Received { .. } | Event::Reset => None,
        };
        match (s.in_session, completed) {
            (false, Some(Some(SessionSelection::NonDefault))) => {
                s.in_session = true; // UDSS_LLR_0159
                s.s3.start(now, s.reload);
            }
            (true, Some(Some(SessionSelection::Default))) => s.leave(), // UDSS_LLR_0163
            (true, Some(_)) => s.s3.start(now, s.reload),               // UDSS_LLR_0161
            _ => {}
        }
    }

    fn expire(&mut self, _: Timestamp) -> bool {
        false
    }

    fn expire_channel(s: &mut PhysicalSession, now: Timestamp) -> bool {
        let due = s.in_session && s.s3.expired(now);
        if due {
            s.s3.stop(); // UDSS_LLR_0162
        }
        due
    }

    fn deadline(&self) -> Option<Timestamp> {
        None
    }

    fn channel_deadline(s: &PhysicalSession) -> Option<Timestamp> {
        s.s3.deadline()
    }
}
