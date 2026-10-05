//! The client's keep-alive modes, ``UDSS_LLR_0149`` to ``UDSS_LLR_0163``.

pub(crate) use role::PhysicalSession;

mod role {
    /// What a keep-alive mode keeps in each physical channel. Public in a private module,
    /// so it can bound [`super::KeepAliveMode`] and no caller can name or implement it.
    pub trait Role: core::fmt::Debug {
        /// The mode's state in each physical channel's storage (``UDSS_LLR_0151``).
        type Channel: Copy + core::fmt::Debug;
    }

    /// ``UDSS_LLR_0151`` — a physical channel's `tS3_Client`, its reload and its
    /// session fact.
    #[derive(Debug, Clone, Copy)]
    #[expect(dead_code, reason = "read when physical keep-alive starts tS3_Client")]
    pub struct PhysicalSession {
        pub(crate) reload: u32,
        pub(crate) s3: crate::timer::Timer<crate::timer::Reaches>,
        pub(crate) in_session: bool,
    }

    impl PhysicalSession {
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
}

impl FunctionalKeepAlive {
    /// The initial state, with the client-wide `tS3_Client` reload.
    ///
    /// ``UDSS_LLR_0153`` — no session is kept alive and the timer is not running.
    /// ``UDSS_LLR_0152`` — `s3_client` must cover the longest path among every server the
    /// functional address reaches. [`crate::Client::set_keep_alive_reload`] sets it again.
    #[must_use]
    pub const fn new(s3_client: u32) -> Self {
        Self { reload: s3_client }
    }
}

impl crate::sealed::Sealed for FunctionalKeepAlive {}
impl KeepAliveMode for FunctionalKeepAlive {}

impl role::Role for FunctionalKeepAlive {
    type Channel = ();
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
}
