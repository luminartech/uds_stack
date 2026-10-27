//! The C tables, as `include/uds_c.h` lays them out, and the lookups over them.
//!
//! Every handler call goes through a method here, so the one `unsafe` contract that
//! covers calling C, [`Layers::new`]'s, is relied on in this module only.

#![allow(unsafe_code, reason = "reads the C tables and calls the C handlers")]

use core::slice;

use uds_services::{
    Access, DiagnosticSessionType as Session, Levels, SecurityLevel, Sessions,
};

use crate::UDS_DTC_ALL;

/// A DTC number's three bytes, `DTCHighByte` to `DTCLowByte`: the memory's `uint32_t`
/// carries no fourth.
const DTC_NUMBER: u32 = 0x00FF_FFFF;

/// `UdsDidReadFn`.
pub type UdsDidReadFn = unsafe extern "C" fn(op: u8) -> u8;
/// `UdsDidWriteFn`.
pub type UdsDidWriteFn = unsafe extern "C" fn(op: u8, data: *const u8, len: usize) -> u8;
/// `UdsRoutineFn`.
pub type UdsRoutineFn =
    unsafe extern "C" fn(op: u8, option: *const u8, option_len: usize) -> u8;
/// `UdsSeedFn`.
pub type UdsSeedFn =
    unsafe extern "C" fn(op: u8, option: *const u8, option_len: usize) -> u8;
/// `UdsKeyFn`.
pub type UdsKeyFn = unsafe extern "C" fn(op: u8, key: *const u8, key_len: usize) -> u8;
/// `UdsDtcGetFn`.
pub type UdsDtcGetFn =
    unsafe extern "C" fn(index: usize, dtc: *mut u32, status: *mut u8) -> bool;
/// `UdsDtcSnapshotCountFn`.
pub type UdsDtcSnapshotCountFn = unsafe extern "C" fn(index: usize) -> usize;
/// `UdsDtcSnapshotItemFn`.
pub type UdsDtcSnapshotItemFn = unsafe extern "C" fn(
    index: usize,
    item: usize,
    did: *mut u16,
    data: *mut *const u8,
    len: *mut usize,
) -> bool;
/// `UdsDtcClearFn`.
pub type UdsDtcClearFn = unsafe extern "C" fn(op: u8, index: usize) -> u8;
/// `UdsDtcSettingFn`.
pub type UdsDtcSettingFn = unsafe extern "C" fn(op: u8, setting_type: u8) -> u8;
/// `UdsCommControlFn`.
pub type UdsCommControlFn =
    unsafe extern "C" fn(op: u8, control_type: u8, communication_type: u8) -> u8;
/// `UdsTransferBeginFn`.
pub type UdsTransferBeginFn =
    unsafe extern "C" fn(op: u8, address: u32, size: u32, data_format: u8) -> u8;
/// `UdsTransferBlockFn`.
pub type UdsTransferBlockFn =
    unsafe extern "C" fn(op: u8, data: *const u8, len: usize) -> u8;
/// `UdsTransferFinishFn`.
pub type UdsTransferFinishFn = unsafe extern "C" fn(op: u8) -> u8;
/// `UdsTransitionCheckFn`.
pub type UdsTransitionCheckFn =
    unsafe extern "C" fn(active_session: u8, sub_function: u8) -> u8;
/// `UdsTransitionPrepareFn`.
pub type UdsTransitionPrepareFn =
    unsafe extern "C" fn(op: u8, active_session: u8, sub_function: u8) -> u8;
/// `UdsTransitionExecuteFn`.
pub type UdsTransitionExecuteFn = unsafe extern "C" fn(sub_function: u8);
/// `UdsTransitionAbandonFn`.
pub type UdsTransitionAbandonFn =
    unsafe extern "C" fn(active_session: u8, sub_function: u8);

/// `UdsAccess`: the sessions an operation is available in, as `UDS_SESSION_MASK_*`
/// bits, and the security level, by its requestSeed value, that unlocks it, 0 for none.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdsAccess {
    /// `UDS_SESSION_MASK_*` bits.
    pub sessions: u8,
    /// The requestSeed sub-function of the level that unlocks it, or 0.
    pub security_level: u8,
}

/// `UdsDidEntry`: one data identifier.
#[repr(C)]
#[derive(Debug)]
pub struct UdsDidEntry {
    /// The identifier.
    pub did: u16,
    /// `UDS_SESSION_MASK_*` bits: where a read is served.
    pub read_sessions: u8,
    /// The read handler; `None` where the identifier is not readable.
    pub read: Option<UdsDidReadFn>,
    /// Where a write is served, and the level that unlocks it.
    pub write_access: UdsAccess,
    /// The record a read answers and a write carries, in bytes.
    pub len: u16,
    /// The write handler; `None` where the identifier is not writable.
    pub write: Option<UdsDidWriteFn>,
}

/// `UdsRidEntry`: one routine.
#[repr(C)]
#[derive(Debug)]
pub struct UdsRidEntry {
    /// The identifier.
    pub rid: u16,
    /// The longest status record a step answers, in bytes.
    pub status_len: u16,
    /// Where the routine is served, and the level that unlocks it.
    pub access: UdsAccess,
    /// startRoutine; `None` where not supported.
    pub start: Option<UdsRoutineFn>,
    /// stopRoutine; `None` where not supported.
    pub stop: Option<UdsRoutineFn>,
    /// requestRoutineResults; `None` where not supported.
    pub results: Option<UdsRoutineFn>,
}

/// `UdsSecurityLevel`: one security level.
#[repr(C)]
#[derive(Debug)]
pub struct UdsSecurityLevel {
    /// The requestSeed sub-function, odd.
    pub level: u8,
    /// `UDS_SESSION_MASK_*` bits: where the level may be unlocked.
    pub sessions: u8,
    /// Failed keys before the delay.
    pub attempt_limit: u8,
    /// The seed's length, in bytes.
    pub seed_len: u8,
    /// The lockout after `attempt_limit` failed keys, in milliseconds.
    pub delay_ms: u32,
    /// The seed handler.
    pub seed: Option<UdsSeedFn>,
    /// The key handler.
    pub key: Option<UdsKeyFn>,
}

/// `UdsDtcHooks`: a layer's DTC memory and handlers, `None` where the layer serves none.
#[repr(C)]
#[derive(Debug)]
pub struct UdsDtcHooks {
    /// `ReadDTCInformation` (0x19): each DTC's number and status.
    pub get: Option<UdsDtcGetFn>,
    /// The number of identifiers in a DTC's snapshot record.
    pub snapshot_count: Option<UdsDtcSnapshotCountFn>,
    /// One identifier of a DTC's snapshot record, and its data.
    pub snapshot_item: Option<UdsDtcSnapshotItemFn>,
    /// `ClearDiagnosticInformation` (0x14).
    pub clear: Option<UdsDtcClearFn>,
    /// `ControlDTCSetting` (0x85).
    pub setting: Option<UdsDtcSettingFn>,
}

/// `UdsTransferHooks`: a layer's download handlers, `None` where the layer serves none.
#[repr(C)]
#[derive(Debug)]
pub struct UdsTransferHooks {
    /// Where a download is served, and the level that unlocks it.
    pub access: UdsAccess,
    /// `RequestDownload` (0x34).
    pub begin: Option<UdsTransferBeginFn>,
    /// `TransferData` (0x36).
    pub block: Option<UdsTransferBlockFn>,
    /// `RequestTransferExit` (0x37).
    pub finish: Option<UdsTransferFinishFn>,
}

/// `UdsTransitionHooks`: a layer's say in one service's transitions. A `None` hook
/// behaves as an empty one, a check or prepare answering positive at once.
#[repr(C)]
#[derive(Debug)]
pub struct UdsTransitionHooks {
    /// Whether the transition may be made.
    pub check: Option<UdsTransitionCheckFn>,
    /// Brings the system to where the transition can be made.
    pub prepare: Option<UdsTransitionPrepareFn>,
    /// Makes the transition, once `uds_tcp_write` has taken its response.
    pub execute: Option<UdsTransitionExecuteFn>,
    /// Undoes the prepare of a transition that is not made.
    pub abandon: Option<UdsTransitionAbandonFn>,
}

/// `UdsLayer`: one layer of services, a table per identifier-based service and handlers
/// for the others.
#[repr(C)]
#[derive(Debug)]
pub struct UdsLayer {
    /// `did_count` entries, or NULL for none.
    pub dids: *const UdsDidEntry,
    /// The number of entries at `dids`.
    pub did_count: usize,
    /// `rid_count` entries, or NULL for none.
    pub rids: *const UdsRidEntry,
    /// The number of entries at `rids`.
    pub rid_count: usize,
    /// `security_level_count` entries, or NULL for none.
    pub security_levels: *const UdsSecurityLevel,
    /// The number of entries at `security_levels`.
    pub security_level_count: usize,
    /// The DTC services.
    pub dtc: UdsDtcHooks,
    /// `CommunicationControl` (0x28).
    pub comm_control: Option<UdsCommControlFn>,
    /// The download services.
    pub transfer: UdsTransferHooks,
    /// `DiagnosticSessionControl` (0x10).
    pub session: UdsTransitionHooks,
    /// `EcuReset` (0x11).
    pub reset: UdsTransitionHooks,
}

// SAFETY: a layer is plain data. Sharing one shares its pointer values only; what they
// point to is read, and its handlers called, through `Layers` alone, under
// `Layers::new`'s contract. Lets firmware list `extern` layers in a `static`.
unsafe impl Sync for UdsLayer {}

#[cfg(target_pointer_width = "32")]
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<UdsAccess>() == 2);
    assert!(size_of::<UdsDidEntry>() == 16 && offset_of!(UdsDidEntry, len) == 10);
    assert!(size_of::<UdsRidEntry>() == 20 && offset_of!(UdsRidEntry, start) == 8);
    assert!(
        size_of::<UdsSecurityLevel>() == 16 && offset_of!(UdsSecurityLevel, delay_ms) == 4
    );
    assert!(size_of::<UdsDtcHooks>() == 20);
    assert!(size_of::<UdsTransferHooks>() == 16);
    assert!(size_of::<UdsTransitionHooks>() == 16);
    assert!(size_of::<UdsLayer>() == 96);
    assert!(offset_of!(UdsLayer, dtc) == 24 && offset_of!(UdsLayer, transfer) == 48);
    assert!(offset_of!(UdsLayer, session) == 64 && offset_of!(UdsLayer, reset) == 80);
};

/// `UDS_SESSION_MASK_*`: each session's bit in an access mask.
const SESSION_BITS: [(u8, Session); 3] = [
    (0x01, Session::DefaultSession),
    (0x02, Session::ProgrammingSession),
    (0x04, Session::ExtendedDiagnosticSession),
];

/// The sessions a `UDS_SESSION_MASK_*` mask names.
pub(crate) fn sessions(mask: u8) -> Sessions {
    SESSION_BITS
        .into_iter()
        .filter(|&(bit, _)| mask & bit != 0)
        .fold(Sessions::NONE, |sessions, (_, session)| {
            sessions.with(session)
        })
}

impl UdsAccess {
    /// The access this entry grants. A security level that is not a requestSeed level
    /// grants none, so a table error fails closed; [`crate::TableApp::validate`] refuses
    /// such a table at start-up.
    pub(crate) fn access(self) -> Access {
        let access = Access::new(sessions(self.sessions));
        match (
            self.security_level,
            SecurityLevel::from_request_seed(self.security_level),
        ) {
            (0, _) => access,
            (_, Some(level)) => access.unlocked_by(Levels::NONE.with(level)),
            (_, None) => Access::new(Sessions::NONE),
        }
    }

    /// Whether the security level is none or a requestSeed level.
    pub(crate) fn valid(self) -> bool {
        self.security_level == 0
            || SecurityLevel::from_request_seed(self.security_level).is_some()
    }
}

/// The layers a server serves, searched in order: an earlier layer's entry wins, so a
/// later layer adds services but never replaces one.
///
/// Built once, by [`Layers::new`], whose contract is what lets every lookup and handler
/// call after it be safe.
#[derive(Debug, Clone, Copy)]
pub struct Layers(&'static [&'static UdsLayer]);

impl Layers {
    /// The layers `layers` lists, searched in that order.
    ///
    /// # Safety
    ///
    /// For the rest of the program's run:
    ///
    /// - every table a layer points to holds its stated count of initialized entries,
    ///   and nothing changes a layer or its tables;
    /// - every handler keeps the contract `include/uds_c.h` states for it, and the
    ///   `uds_record_copy` it calls is this crate's;
    /// - the server, its [`Firmware`](crate::Firmware) and every other use of these
    ///   layers run in one execution context, so no handler is entered twice at once;
    /// - the program runs one server: the record lent to a handler and the transitions
    ///   awaiting their responses are kept in this crate's statics, which a second
    ///   server would share.
    #[must_use]
    pub const unsafe fn new(layers: &'static [&'static UdsLayer]) -> Self {
        Self(layers)
    }

    /// Every layer, in order.
    pub(crate) fn iter(self) -> impl Iterator<Item = &'static UdsLayer> {
        self.0.iter().copied()
    }

    /// Every layer's table of `T`, in order.
    fn tables<T: 'static>(
        self,
        of: fn(&UdsLayer) -> (*const T, usize),
    ) -> impl Iterator<Item = &'static T> {
        self.iter().flat_map(move |layer| {
            let (entries, count) = of(layer);
            if entries.is_null() {
                &[]
            } else {
                // SAFETY: `Layers::new`: a non-NULL table holds `count` initialized
                // entries that live, unchanged, for the rest of the run.
                unsafe { slice::from_raw_parts(entries, count) }
            }
        })
    }

    pub(crate) fn dids(self) -> impl Iterator<Item = &'static UdsDidEntry> {
        self.tables(|l| (l.dids, l.did_count))
    }

    pub(crate) fn rids(self) -> impl Iterator<Item = &'static UdsRidEntry> {
        self.tables(|l| (l.rids, l.rid_count))
    }

    pub(crate) fn security_levels(self) -> impl Iterator<Item = &'static UdsSecurityLevel> {
        self.tables(|l| (l.security_levels, l.security_level_count))
    }

    /// Every layer's download access, in order.
    pub(crate) fn transfer_accesses(self) -> impl Iterator<Item = UdsAccess> {
        self.iter().map(|l| l.transfer.access)
    }

    /// Every layer's download handlers, in order.
    pub(crate) fn transfer_hooks(self) -> impl Iterator<Item = &'static UdsTransferHooks> {
        self.iter().map(|l| &l.transfer)
    }

    /// Every layer's DTC memory and handlers, in order.
    pub(crate) fn dtc_hooks(self) -> impl Iterator<Item = &'static UdsDtcHooks> {
        self.iter().map(|l| &l.dtc)
    }

    /// The first entry for `did`.
    pub(crate) fn did(self, did: u16) -> Option<&'static UdsDidEntry> {
        self.dids().find(|e| e.did == did)
    }

    /// The first entry for `rid`.
    pub(crate) fn rid(self, rid: u16) -> Option<&'static UdsRidEntry> {
        self.rids().find(|e| e.rid == rid)
    }

    /// The first level whose requestSeed is `level`, with its index among every
    /// layer's levels.
    pub(crate) fn security_level(
        self,
        level: u8,
    ) -> Option<(usize, &'static UdsSecurityLevel)> {
        self.security_levels()
            .enumerate()
            .find(|(_, e)| e.level == level)
    }

    /// The first layer's handler `of` names.
    fn first<T>(self, of: impl Fn(&'static UdsLayer) -> Option<T>) -> Option<T> {
        self.iter().find_map(of)
    }

    /// The DTC memory of the first layer that keeps one, with that layer's clear.
    pub(crate) fn dtc_memory(self) -> Option<DtcMemory> {
        self.first(|l| {
            l.dtc.get.map(|get| DtcMemory {
                get,
                snapshot_count: l.dtc.snapshot_count,
                snapshot_item: l.dtc.snapshot_item,
                clear: l.dtc.clear,
            })
        })
    }

    pub(crate) fn dtc_setting(self) -> Option<Handler<UdsDtcSettingFn>> {
        self.first(|l| l.dtc.setting).map(Handler)
    }

    pub(crate) fn comm_control(self) -> Option<Handler<UdsCommControlFn>> {
        self.first(|l| l.comm_control).map(Handler)
    }

    /// The download handlers of the first layer that serves downloads.
    pub(crate) fn transfer(self) -> Option<&'static UdsTransferHooks> {
        self.first(|l| Some(&l.transfer).filter(|t| t.begin.is_some()))
    }

    /// Every layer's check of the transition, in order.
    pub(crate) fn checks(
        self,
        hooks: fn(&UdsLayer) -> &UdsTransitionHooks,
    ) -> impl Iterator<Item = Handler<UdsTransitionCheckFn>> {
        self.iter().filter_map(move |l| hooks(l).check.map(Handler))
    }

    /// Every layer's prepare of the transition, in order: `None` for a layer without
    /// one.
    pub(crate) fn prepares(
        self,
        hooks: fn(&UdsLayer) -> &UdsTransitionHooks,
    ) -> impl Iterator<Item = Option<Handler<UdsTransitionPrepareFn>>> {
        self.iter().map(move |l| hooks(l).prepare.map(Handler))
    }

    /// The abandon of the transition from `active` to `sub_function` by the first
    /// `count` layers, the last of them first.
    pub(crate) fn abandon(
        self,
        hooks: fn(&UdsLayer) -> &UdsTransitionHooks,
        count: usize,
        active: u8,
        sub_function: u8,
    ) {
        for abandon in self
            .0
            .iter()
            .take(count)
            .rev()
            .filter_map(|l| hooks(l).abandon)
        {
            // SAFETY: `Layers::new`: a handler with the header's signature.
            unsafe { abandon(active, sub_function) };
        }
    }

    /// Every layer's execute of the transition to `sub_function`, the last layer's
    /// first.
    pub(crate) fn execute(
        self,
        hooks: fn(&UdsLayer) -> &UdsTransitionHooks,
        sub_function: u8,
    ) {
        for execute in self.0.iter().rev().filter_map(|l| hooks(l).execute) {
            // SAFETY: `Layers::new`: a handler with the header's signature.
            unsafe { execute(sub_function) };
        }
    }
}

/// A layer's DTC memory, read through its `UdsDtcHooks`, and so covered by
/// [`Layers::new`]'s contract.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DtcMemory {
    get: UdsDtcGetFn,
    snapshot_count: Option<UdsDtcSnapshotCountFn>,
    snapshot_item: Option<UdsDtcSnapshotItemFn>,
    clear: Option<UdsDtcClearFn>,
}

impl DtcMemory {
    /// DTC `index`'s number, its three bytes, and status; `None` past the last DTC.
    pub(crate) fn get(self, index: usize) -> Option<(u32, u8)> {
        let (mut dtc, mut status) = (0, 0);
        // SAFETY: `Layers::new`: a handler with the header's signature; both
        // out-pointers are to locals.
        unsafe { (self.get)(index, &raw mut dtc, &raw mut status) }
            .then_some((dtc & DTC_NUMBER, status))
    }

    /// Every DTC, in index order: its index, number and status. The indices stop short
    /// of `UDS_DTC_ALL`, which names every DTC to the clear.
    pub(crate) fn dtcs(self) -> impl Iterator<Item = (usize, u32, u8)> {
        (0..UDS_DTC_ALL).map_while(move |index| {
            self.get(index).map(|(dtc, status)| (index, dtc, status))
        })
    }

    /// The clear of the layer that keeps the memory, so its indices are the memory's.
    pub(crate) fn clear(self) -> Option<Handler<UdsDtcClearFn>> {
        self.clear.map(Handler)
    }

    /// The number of identifiers in DTC `index`'s snapshot record, zero where it holds
    /// none.
    pub(crate) fn snapshot_count(self, index: usize) -> usize {
        // SAFETY: `Layers::new`: a handler with the header's signature.
        self.snapshot_count
            .map_or(0, |count| unsafe { count(index) })
    }

    /// Calls `with` on identifier `item` of DTC `index`'s snapshot record and its data,
    /// `None` where the memory has no such item.
    pub(crate) fn snapshot_item<R>(
        self,
        index: usize,
        item: usize,
        with: impl FnOnce(u16, &[u8]) -> R,
    ) -> Option<R> {
        let read = self.snapshot_item?;
        let (mut did, mut data, mut len) = (0, core::ptr::null(), 0);
        // SAFETY: `Layers::new`: a handler with the header's signature; the out-pointers
        // are to locals.
        let found = unsafe { read(index, item, &raw mut did, &raw mut data, &raw mut len) };
        if !found {
            return None;
        }
        let data: &[u8] = if len == 0 {
            &[]
        } else if data.is_null() {
            return None;
        } else {
            // SAFETY: the header's contract: `data` points to `len` readable bytes that
            // stay valid until the server's next call into the layer, after `with`
            // returns.
            unsafe { core::slice::from_raw_parts(data, len) }
        };
        Some(with(did, data))
    }
}

/// A C handler read from a [`Layers`], and so covered by [`Layers::new`]'s contract.
///
/// Several of the header's handler types share a signature, so the calls are given per
/// signature rather than per handler type.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Handler<F>(pub(crate) F);

impl Handler<unsafe extern "C" fn(u8) -> u8> {
    pub(crate) fn call(self, a: u8) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header.
        unsafe { (self.0)(a) }
    }
}

impl Handler<unsafe extern "C" fn(u8, u8) -> u8> {
    pub(crate) fn call(self, a: u8, b: u8) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header.
        unsafe { (self.0)(a, b) }
    }
}

impl Handler<unsafe extern "C" fn(u8, u8, u8) -> u8> {
    pub(crate) fn call(self, a: u8, b: u8, c: u8) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header.
        unsafe { (self.0)(a, b, c) }
    }
}

impl Handler<unsafe extern "C" fn(u8, usize) -> u8> {
    pub(crate) fn call(self, op: u8, value: usize) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header.
        unsafe { (self.0)(op, value) }
    }
}

impl Handler<unsafe extern "C" fn(u8, u32, u32, u8) -> u8> {
    pub(crate) fn call(self, op: u8, address: u32, size: u32, data_format: u8) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header.
        unsafe { (self.0)(op, address, size, data_format) }
    }
}

/// A DID write, a routine step, a seed, a key and a download block: the handlers given
/// a request's bytes.
impl Handler<unsafe extern "C" fn(u8, *const u8, usize) -> u8> {
    pub(crate) fn call(self, op: u8, bytes: &[u8]) -> u8 {
        // SAFETY: `Layers::new`: a handler with this signature in the header, which
        // reads at most `len` bytes from the pointer, and `bytes` is that many.
        unsafe { (self.0)(op, bytes.as_ptr(), bytes.len()) }
    }
}

#[cfg(test)]
mod tests {
    use uds_services::{DiagnosticSessionType as Session, Levels, SecurityLevel};

    use super::*;

    #[test]
    fn a_session_mask_names_its_sessions() {
        let extended = sessions(0x04);
        assert!(extended.contains(Session::ExtendedDiagnosticSession));
        assert!(!extended.contains(Session::DefaultSession));
        assert!(!extended.contains(Session::ProgrammingSession));
        let all = sessions(0x07);
        assert!(
            all.contains(Session::DefaultSession)
                && all.contains(Session::ProgrammingSession)
        );
        assert!(sessions(0x00).is_empty());
    }

    #[test]
    fn an_access_needs_its_level_only_where_one_is_set() {
        let open = UdsAccess {
            sessions: 0x04,
            security_level: 0,
        };
        assert_eq!(open.access().levels(), None);
        let locked = UdsAccess {
            sessions: 0x04,
            security_level: 0x61,
        };
        assert_eq!(
            locked.access().levels(),
            SecurityLevel::from_request_seed(0x61).map(|l| Levels::NONE.with(l))
        );
    }

    #[test]
    fn an_access_naming_a_send_key_level_grants_nothing() {
        let typo = UdsAccess {
            sessions: 0x07,
            security_level: 0x02,
        };
        assert!(!typo.valid());
        assert!(typo.access().sessions().is_empty());
    }
}
