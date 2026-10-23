//! The application a server dispatches to: every identifier-based service looks its
//! identifier up in the layers and calls the handler the entry names, and every other
//! service calls the first layer that serves it. A new identifier is a table entry and a
//! handler in C, with no change here. Resets and session changes go through every
//! layer's transition hooks. What the integrator decides comes from the [`Policy`].

mod comm;
mod data;
mod dtc;
mod routine;
mod security;
mod transfer;

use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU16, Ordering};

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, AfterReset, DiagnosticSessionControl, DiagnosticSessionType as Session,
    EcuReset, ResetType, SecurityLevel, SessionTiming, SessionTransition, Sessions,
    TesterPresent,
};

use crate::layer::{Layers, UdsLayer, UdsSecurityLevel, UdsTransitionHooks};
use crate::op;
use crate::record::Record;

pub use data::{ACTIVE_SESSION_DID, Did};
pub use routine::Rid;

/// What the integrator decides: which layers the server serves, the sessions and their
/// timing, and how large a request or a record may be.
///
/// Implemented on a type of the integrator's own, which names the application as
/// `TableApp<ThatType>`. Every limit has a default, and each is folded into the
/// server's buffers by `uds_server!`.
pub trait Policy: 'static {
    /// The layers the server serves.
    const LAYERS: &'static Layers;

    /// The sessions this server supports at all (ISO 14229-1:2020 Table 25). A request
    /// for another is answered `subFunctionNotSupported` (0x12).
    const SESSIONS: Sessions;

    /// The sessions `RequestDownload` and the transfer it opens are served in.
    const TRANSFER_SESSIONS: Sessions = Sessions::NONE.with(Session::ProgrammingSession);

    /// The most DIDs one `ReadDataByIdentifier` request may name.
    const MAX_DIDS_PER_REQUEST: usize = 4;
    /// The longest DID record a table entry may declare.
    const MAX_DID_LEN: usize = 64;
    /// The longest routine status record a table entry may declare.
    const MAX_STATUS_LEN: usize = 64;
    /// The longest routine option record a request may carry.
    const MAX_OPTION_LEN: usize = 32;
    /// The longest seed a security level may declare.
    const MAX_SEED_LEN: usize = 32;
    /// The longest key a `sendKey` may carry.
    const MAX_KEY_LEN: usize = 64;
    /// The longest `securityAccessDataRecord` a `requestSeed` may carry.
    const MAX_SECURITY_RECORD_LEN: usize = 4;
    /// The most DTCs one `ReadDTCInformation` response may carry.
    const MAX_DTCS: usize = 64;
    /// The longest `TransferData` block.
    const MAX_BLOCK_LEN: usize = 1_024;

    /// The `P2Server_max` and `P2*Server_max` that `session` advertises and, while it is
    /// in force, the server keeps.
    fn timing(session: Session) -> SessionTiming;

    /// Whether `session` may be entered from `active`; a refusal is answered
    /// `subFunctionNotSupportedInActiveSession` (0x7E). Every transition by default.
    #[must_use]
    fn supported_from(session: Session, active: Session) -> bool {
        let _ = (session, active);
        true
    }

    /// Whether entering `session` leaves the software this server runs, as a jump to a
    /// bootloader does; its positive response then closes the connection. None by
    /// default.
    #[must_use]
    fn leaves_running_software(session: Session) -> bool {
        let _ = session;
        false
    }

    /// Where the `EcuReset` of `kind` is available and which levels unlock it, or `None`
    /// where it is not supported. Every reset admitted restarts the server once its
    /// response is confirmed. `hardReset` in every session by default.
    #[must_use]
    fn reset_access(kind: ResetType) -> Option<Access> {
        (kind == ResetType::HardReset).then_some(Access::new(Sessions::ALL))
    }
}

/// The longest record any handler of an application under `P` answers with: a DID
/// record, a routine status, a seed or a DTC report. [`TableApp`]'s `RECORD` must be at
/// least this.
#[must_use]
pub const fn record_len<P: Policy>() -> usize {
    let dtc_report = dtc::report_len(P::MAX_DTCS);
    let mut len = P::MAX_DID_LEN;
    if P::MAX_STATUS_LEN > len {
        len = P::MAX_STATUS_LEN;
    }
    if P::MAX_SEED_LEN > len {
        len = P::MAX_SEED_LEN;
    }
    if dtc_report > len {
        len = dtc_report;
    }
    len
}

/// A table that [`TableApp::validate`] refuses: an entry the server could not serve
/// safely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    /// A DID's record is longer than [`Policy::MAX_DID_LEN`].
    #[error("DID {did:#06X}: its record is longer than the policy's MAX_DID_LEN")]
    DidTooLong {
        /// The identifier.
        did: u16,
    },
    /// A routine's status record is longer than [`Policy::MAX_STATUS_LEN`].
    #[error("routine {rid:#06X}: its status is longer than the policy's MAX_STATUS_LEN")]
    StatusTooLong {
        /// The identifier.
        rid: u16,
    },
    /// A security level's seed is empty or longer than [`Policy::MAX_SEED_LEN`].
    #[error("security level {level:#04X}: its seed is empty or longer than MAX_SEED_LEN")]
    SeedLength {
        /// The level's requestSeed value.
        level: u8,
    },
    /// A security level is not a requestSeed sub-function.
    #[error("security level {level:#04X}: not a requestSeed sub-function")]
    NotRequestSeed {
        /// The value the table gives.
        level: u8,
    },
    /// The layers define more security levels than the application tracks.
    #[error("{count} security levels, more than the application's LEVELS")]
    TooManyLevels {
        /// How many the layers define.
        count: usize,
    },
    /// An access names a security level that is not a requestSeed sub-function.
    #[error("{0}: its access names a level that is not a requestSeed sub-function")]
    Access(AccessOf),
    /// An entry lacks a handler it needs: a DID neither read nor written, a routine
    /// with no step, a level without its seed or key, or a download without its
    /// block or finish. A zero-filled table entry is one.
    #[error("{0}: a handler it needs is missing")]
    NoHandler(AccessOf),
    /// An identifier listed twice, in one layer or two, or a DID the server answers
    /// itself ([`ACTIVE_SESSION_DID`]): only the first entry would ever be served.
    #[error("{0}: listed twice, so only its first entry is served")]
    Duplicate(AccessOf),
    /// A security level that would lock out before any key is tried.
    #[error("security level {level:#04X}: its attempt limit is 0")]
    NoAttempts {
        /// The level's requestSeed value.
        level: u8,
    },
}

/// The entry a [`TableError`] names.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessOf {
    /// A DID.
    Did(u16),
    /// A routine.
    Routine(u16),
    /// A layer's download handlers, by the layer's index.
    Transfer(usize),
    /// A security level, by its requestSeed value.
    Level(u8),
}

impl fmt::Display for AccessOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Did(did) => write!(f, "DID {did:#06X}"),
            Self::Routine(rid) => write!(f, "routine {rid:#06X}"),
            Self::Transfer(layer) => write!(f, "layer {layer}'s download"),
            Self::Level(level) => write!(f, "security level {level:#04X}"),
        }
    }
}

/// The first value the iterators `items` makes hold twice.
fn first_duplicate<T: PartialEq + Copy, I: Iterator<Item = T>>(
    items: impl Fn() -> I,
) -> Option<T> {
    items()
        .enumerate()
        .find(|&(index, item)| items().skip(index).skip(1).any(|other| other == item))
        .map(|(_, item)| item)
}

/// The application: the services of [`Policy::LAYERS`]' tables, with the session and
/// security state the handlers' answers depend on.
///
/// `RECORD` is the buffer every response record is lent from, at least
/// [`record_len`]`::<P>()`, which the default covers for the default limits; a shorter one
/// fails to compile. `LEVELS` is how many security levels it counts attempts and
/// delays for; [`TableApp::validate`] refuses layers that define more.
pub struct TableApp<P, const RECORD: usize = 257, const LEVELS: usize = 4> {
    session: Session,
    /// Failed keys per security level, by its index among the layers' levels.
    attempts: [u8; LEVELS],
    /// When each level's delay ends, in milliseconds on the server's clock.
    delay_until: [Option<u64>; LEVELS],
    /// The record lent to the handler being called.
    record: [u8; RECORD],
    policy: PhantomData<fn() -> P>,
}

impl<P, const RECORD: usize, const LEVELS: usize> fmt::Debug
    for TableApp<P, RECORD, LEVELS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableApp")
            .field("session", &self.session)
            .field("attempts", &self.attempts)
            .field("delay_until", &self.delay_until)
            .finish_non_exhaustive()
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> Default
    for TableApp<P, RECORD, LEVELS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> TableApp<P, RECORD, LEVELS> {
    const RECORD_FITS: () = assert!(
        RECORD >= record_len::<P>(),
        "RECORD is shorter than the longest record the policy allows: see uds_c::record_len"
    );

    /// The application in the default session, every level locked and counting no
    /// failed keys. A `const fn`, so a server holding it is built as a `static`.
    #[must_use]
    pub const fn new() -> Self {
        let () = Self::RECORD_FITS;
        Self {
            session: Session::DefaultSession,
            attempts: [0; LEVELS],
            delay_until: [None; LEVELS],
            record: [0; RECORD],
            policy: PhantomData,
        }
    }

    /// Checks the layers once, before the server starts: every record within the
    /// policy's limits, every security level a requestSeed sub-function allowing at
    /// least one attempt and at most `LEVELS` of them, every access naming such a level
    /// or none, every entry with the handlers it needs, and no identifier twice.
    ///
    /// The server serves tables that fail this without undefined behavior, failing
    /// closed: a record past a limit is answered `generalReject` (0x10), an invalid
    /// access grants nothing, and a level past `LEVELS` is not supported.
    ///
    /// # Errors
    ///
    /// The first entry the server cannot serve safely, as a [`TableError`].
    pub fn validate() -> Result<(), TableError> {
        let layers = P::LAYERS;
        if let Some(e) = layers.dids().find(|e| usize::from(e.len) > P::MAX_DID_LEN) {
            return Err(TableError::DidTooLong { did: e.did });
        }
        if let Some(e) = layers
            .rids()
            .find(|e| usize::from(e.status_len) > P::MAX_STATUS_LEN)
        {
            return Err(TableError::StatusTooLong { rid: e.rid });
        }
        if let Some(e) = layers
            .security_levels()
            .find(|e| SecurityLevel::from_request_seed(e.level).is_none())
        {
            return Err(TableError::NotRequestSeed { level: e.level });
        }
        if let Some(e) = layers
            .security_levels()
            .find(|e| !(1..=P::MAX_SEED_LEN).contains(&usize::from(e.seed_len)))
        {
            return Err(TableError::SeedLength { level: e.level });
        }
        let count = layers.security_levels().count();
        if count > LEVELS {
            return Err(TableError::TooManyLevels { count });
        }
        if let Some(e) = layers.dids().find(|e| !e.write_access.valid()) {
            return Err(TableError::Access(AccessOf::Did(e.did)));
        }
        if let Some(e) = layers.rids().find(|e| !e.access.valid()) {
            return Err(TableError::Access(AccessOf::Routine(e.rid)));
        }
        if let Some(index) = layers.transfer_accesses().position(|a| !a.valid()) {
            return Err(TableError::Access(AccessOf::Transfer(index)));
        }
        if let Some(e) = layers.security_levels().find(|e| e.attempt_limit == 0) {
            return Err(TableError::NoAttempts { level: e.level });
        }
        if let Some(e) = layers
            .dids()
            .find(|e| e.read.is_none() && e.write.is_none())
        {
            return Err(TableError::NoHandler(AccessOf::Did(e.did)));
        }
        if let Some(e) = layers
            .rids()
            .find(|e| e.start.is_none() && e.stop.is_none() && e.results.is_none())
        {
            return Err(TableError::NoHandler(AccessOf::Routine(e.rid)));
        }
        if let Some(e) = layers
            .security_levels()
            .find(|e| e.seed.is_none() || e.key.is_none())
        {
            return Err(TableError::NoHandler(AccessOf::Level(e.level)));
        }
        if let Some(index) = layers
            .transfer_hooks()
            .position(|t| t.begin.is_some() && (t.block.is_none() || t.finish.is_none()))
        {
            return Err(TableError::NoHandler(AccessOf::Transfer(index)));
        }
        let dids =
            || core::iter::once(ACTIVE_SESSION_DID).chain(layers.dids().map(|e| e.did));
        if let Some(did) = first_duplicate(dids) {
            return Err(TableError::Duplicate(AccessOf::Did(did)));
        }
        if let Some(rid) = first_duplicate(|| layers.rids().map(|e| e.rid)) {
            return Err(TableError::Duplicate(AccessOf::Routine(rid)));
        }
        if let Some(level) = first_duplicate(|| layers.security_levels().map(|e| e.level)) {
            return Err(TableError::Duplicate(AccessOf::Level(level)));
        }
        Ok(())
    }

    /// The record of `len` bytes a handler answers into, where `len` is within the
    /// policy's `max` for it: `generalReject` (0x10) for a table entry
    /// [`TableApp::validate`] refuses.
    fn answer(&mut self, len: usize, max: usize) -> Result<Record<'_>, Nrc> {
        self.record
            .get_mut(..len)
            .filter(|_| len <= max)
            .map(Record::new)
            .ok_or(Nrc::GeneralReject)
    }

    /// The security level whose requestSeed is `level`, with its index, where the index
    /// is one this application counts for.
    fn level(level: SecurityLevel) -> Option<(usize, &'static UdsSecurityLevel)> {
        P::LAYERS
            .security_level(level.request_seed())
            .filter(|&(index, _)| index < LEVELS)
    }
}

/// A transition awaiting its response: `0x100 | sub_function`, or 0 for none. Set by the
/// server's hooks and taken between polls, so outside the server's borrow.
static DEFERRED_SESSION: AtomicU16 = AtomicU16::new(0);
static DEFERRED_RESET: AtomicU16 = AtomicU16::new(0);

/// Marks a slot's value as set, so a sub-function of 0 is still one.
const DEFERRED: u16 = 0x100;

fn defer(slot: &AtomicU16, sub_function: u8) {
    slot.store(DEFERRED | u16::from(sub_function), Ordering::Relaxed);
}

/// The deferred sub-function, if any, emptying the slot. Loaded and stored rather than
/// swapped: one execution context, and a core without compare-and-swap.
fn take(slot: &AtomicU16) -> Option<u8> {
    let value = slot.load(Ordering::Relaxed);
    slot.store(0, Ordering::Relaxed);
    let [set, sub_function] = value.to_be_bytes();
    (set != 0).then_some(sub_function)
}

fn session_hooks(layer: &UdsLayer) -> &UdsTransitionHooks {
    &layer.session
}

fn reset_hooks(layer: &UdsLayer) -> &UdsTransitionHooks {
    &layer.reset
}

/// Makes the transitions whose responses the server has sent since the last call: each
/// layer's session execute, the last layer's first, for a session the server entered,
/// then each layer's reset execute for a reset it confirmed.
///
/// Called between polls of the server, before each one, so the response has left the
/// socket a poll before its transition is made. The session's execute runs as well when
/// `tS3_Server` returns the server to the default session, with no response.
pub fn execute_deferred(layers: &Layers) {
    if let Some(session) = take(&DEFERRED_SESSION) {
        layers.execute(session_hooks, session);
    }
    if let Some(kind) = take(&DEFERRED_RESET) {
        layers.execute(reset_hooks, kind);
    }
}

/// A handler's result as the service's.
fn settled(nrc: u8) -> Result<(), Nrc> {
    match nrc {
        op::POSITIVE_RESPONSE => Ok(()),
        nrc => Err(Nrc::from(nrc)),
    }
}

/// Every layer's check, then every layer's prepare, of the transition to `sub_function`:
/// the first refusal.
async fn transition(
    layers: &Layers,
    hooks: fn(&UdsLayer) -> &UdsTransitionHooks,
    sub_function: u8,
) -> Result<(), Nrc> {
    for check in layers.checks(hooks) {
        match check.call(sub_function) {
            // A check answers at once; it is never called again to settle.
            op::RESPONSE_PENDING => return Err(Nrc::GeneralReject),
            nrc => settled(nrc)?,
        }
    }
    for prepare in layers.prepares(hooks) {
        settled(op::settle(|op| prepare.call(op, sub_function)).await)?;
    }
    Ok(())
}

/// Available in every session, no level needed.
fn everywhere() -> Access {
    Access::new(Sessions::ALL)
}

/// Available outside the default session, no level needed: ISO 14229-1:2020 Table 23
/// for `CommunicationControl` and `ControlDTCSetting`.
fn outside_default() -> Access {
    Access::new(
        Sessions::NONE
            .with(Session::ExtendedDiagnosticSession)
            .with(Session::ProgrammingSession),
    )
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> DiagnosticSessionControl
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_RESPONSE_LEN: usize = 0;

    fn supports(&self, session: Session) -> bool {
        P::SESSIONS.contains(session)
    }

    fn supported_from(&self, session: Session, active: Session) -> bool {
        P::supported_from(session, active)
    }

    fn timing(&self, session: Session) -> SessionTiming {
        P::timing(session)
    }

    fn leaves_running_software(&self, session: Session) -> bool {
        P::leaves_running_software(session)
    }

    async fn prepare(&mut self, session: Session) -> Result<(), Nrc> {
        transition(P::LAYERS, session_hooks, u8::from(session)).await
    }

    fn on_transition(&mut self, _transition: SessionTransition, entered: Session, _: bool) {
        self.session = entered;
        defer(&DEFERRED_SESSION, u8::from(entered));
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> EcuReset
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;

    fn access(&self, kind: ResetType) -> Option<Access> {
        P::reset_access(kind)
    }

    async fn reset(&mut self, kind: ResetType) -> Result<AfterReset, Nrc> {
        transition(P::LAYERS, reset_hooks, u8::from(kind)).await?;
        Ok(AfterReset::Restart)
    }

    fn confirmed(&mut self, kind: ResetType) {
        defer(&DEFERRED_RESET, u8::from(kind));
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> TesterPresent
    for TableApp<P, RECORD, LEVELS>
{
    fn on_tester_present(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deferred_transition_is_taken_once() {
        let slot = AtomicU16::new(0);
        assert_eq!(take(&slot), None);
        defer(&slot, 0x00);
        assert_eq!(take(&slot), Some(0x00));
        assert_eq!(take(&slot), None);
    }

    #[test]
    fn a_later_deferral_replaces_an_earlier_one() {
        let slot = AtomicU16::new(0);
        defer(&slot, 0x03);
        defer(&slot, 0x01);
        assert_eq!(take(&slot), Some(0x01));
    }
}
