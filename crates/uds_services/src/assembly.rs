//! Assembly, and the const arithmetic it performs.
//!
//! ``UDSSVC_ARCH_0013``: a server is assembled by naming its services, and that list is
//! what `serviceNotSupported` (0x11) is decided from.
//!
//! Assembly is a macro because at its expansion site the types are concrete, so
//! `<Ecu as DataTransfer>::MAX_BLOCK_LEN` is an ordinary const and folding a list of them
//! into [`crate::Storage`]'s array lengths is stable Rust; an associated const of a
//! generic parameter is not usable as an array length.

/// The largest value in `values`, or zero where there are none.
///
/// `#[doc(hidden)]`: macro plumbing, `pub` only because [`crate::uds_server`] expands in
/// the application's crate and names it there, as `__uds_request_bound!` and its siblings
/// are.
#[doc(hidden)]
#[must_use]
pub const fn max_of(mut values: &[usize]) -> usize {
    let mut best = 0;
    while let Some((&first, rest)) = values.split_first() {
        if first > best {
            best = first;
        }
        values = rest;
    }
    best
}

/// The smaller of two values. Caps a derived size at
/// [`crate::UdsTransport::MAX_PDU`].
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is; `core::cmp::min` is not `const`.
#[doc(hidden)]
#[must_use]
pub const fn min2(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

/// The response bound for `ReadDTCInformation`: the widest of `reports`.
///
/// `max_dtcs` bounds the layouts counted in records and `max_record_bytes` those counted
/// in bytes, as [`DtcReportKind::record_len`](crate::DtcReportKind::record_len) tells
/// them apart.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. Saturating: a declared maximum
/// large enough to overflow `usize` is capped by [`min2`] against
/// [`MAX_PDU`](crate::UdsTransport::MAX_PDU) immediately afterwards.
#[doc(hidden)]
#[must_use]
pub const fn dtc_response_bound(
    mut reports: &[crate::DtcReportKind],
    max_dtcs: usize,
    max_record_bytes: usize,
) -> usize {
    let mut best = 0;
    while let Some((&kind, rest)) = reports.split_first() {
        let records = match kind.record_len() {
            Some(width) => max_dtcs.saturating_mul(width),
            None => max_record_bytes,
        };
        let size = kind.header_len().saturating_add(records);
        if size > best {
            best = size;
        }
        reports = rest;
    }
    best
}

/// Whether any of `reports` is bounded in bytes rather than in records.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. `uds_server!` refuses such a
/// server, whose [`ReadDtcInformation::MAX_DTCS`](crate::ReadDtcInformation::MAX_DTCS)
/// cannot bound the response.
#[doc(hidden)]
#[must_use]
pub const fn bounded_in_bytes(mut reports: &[crate::DtcReportKind]) -> bool {
    while let Some((&kind, rest)) = reports.split_first() {
        if kind.record_len().is_none() {
            return true;
        }
        reports = rest;
    }
    false
}

/// The request bound of a client: the widest request among its services.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. The fixed-size requests' widest
/// is `RequestDownload` at 19: the identifier, two format bytes, and an eight-byte
/// address and size, the widest the format identifier admits.
#[doc(hidden)]
#[must_use]
pub const fn client_request_bound(
    limits: crate::ClientLimits,
    max_record_len: usize,
) -> usize {
    max_of(&[
        1_usize.saturating_add(limits.max_dids_per_request.saturating_mul(2)),
        3_usize.saturating_add(max_record_len),
        2_usize.saturating_add(limits.max_key_len),
        4_usize.saturating_add(limits.max_option_len),
        2_usize.saturating_add(limits.max_block_len),
        1_usize.saturating_add(limits.max_option_len),
        19,
    ])
}

/// The response bound of a client: the widest response among its services.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. The fixed-size responses' widest
/// is `RequestDownload`'s at 17: the identifier, the length format byte and up to
/// fifteen bytes of block length.
#[doc(hidden)]
#[must_use]
pub const fn client_response_bound(
    limits: crate::ClientLimits,
    max_record_len: usize,
    max_status_len: usize,
) -> usize {
    use crate::DtcReportKind as K;
    max_of(&[
        1_usize.saturating_add(
            limits
                .max_dids_per_request
                .saturating_mul(2_usize.saturating_add(max_record_len)),
        ),
        dtc_response_bound(
            &[
                K::Count,
                K::DtcList,
                K::FaultDetectionCounters,
                K::SeverityList,
                K::WwhObdSeverity,
                K::SnapshotRecords,
                K::ExtDataRecords,
            ],
            limits.max_dtcs,
            limits.max_dtc_record_len,
        ),
        2_usize.saturating_add(limits.max_seed_len),
        5_usize.saturating_add(max_status_len),
        2_usize.saturating_add(limits.max_block_len),
        1_usize.saturating_add(limits.max_option_len),
        17,
    ])
}

/// Fail const evaluation, naming the request, where a transport whose `MAX_PDU` is
/// `max_pdu` cannot carry a request of `limits` and identifiers of `max_record_len`.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. `uds_client!` evaluates it in a
/// `const`, so a failure is a compile error.
///
/// # Panics
///
/// Where a request of the declared limits is longer than `max_pdu`.
#[doc(hidden)]
pub const fn assert_client_fits(
    limits: crate::ClientLimits,
    max_record_len: usize,
    max_pdu: usize,
) {
    assert!(
        1_usize.saturating_add(2_usize.saturating_mul(limits.max_dids_per_request))
            <= max_pdu,
        "the transport's MAX_PDU cannot carry a read of max_dids_per_request identifiers"
    );
    assert!(
        3_usize.saturating_add(max_record_len) <= max_pdu,
        "the transport's MAX_PDU cannot carry a write of the identifiers' MAX_RECORD_LEN"
    );
    assert!(
        2_usize.saturating_add(limits.max_key_len) <= max_pdu,
        "the transport's MAX_PDU cannot carry a key of max_key_len"
    );
    assert!(
        4_usize.saturating_add(limits.max_option_len) <= max_pdu,
        "the transport's MAX_PDU cannot carry an option record of max_option_len"
    );
    assert!(
        2_usize.saturating_add(limits.max_block_len) <= max_pdu,
        "the transport's MAX_PDU cannot carry a TransferData block of max_block_len"
    );
    assert!(
        19 <= max_pdu,
        "the transport's MAX_PDU cannot carry the widest RequestDownload request"
    );
}

/// Every service a service trait covers, named rather than spelled as a wire byte.
///
/// The byte for each is `uds_protocol`'s to know (``UDSSVC_ARCH_0001`` gives it the
/// format), so this list says only *which services this server implements*, which is
/// the behavior half and the fact ``UDSSVC_ARCH_0013`` needs.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_sids {
    (DiagnosticSessionControl) => {
        [$crate::UdsServiceType::DiagnosticSessionControl]
    };
    (EcuReset) => {
        [$crate::UdsServiceType::EcuReset]
    };
    (ClearDiagnosticInformation) => {
        [$crate::UdsServiceType::ClearDiagnosticInfo]
    };
    (ReadDtcInformation) => {
        [$crate::UdsServiceType::ReadDtcInfo]
    };
    (ReadDataByIdentifier) => {
        [$crate::UdsServiceType::ReadDataByIdentifier]
    };
    (SecurityAccess) => {
        [$crate::UdsServiceType::SecurityAccess]
    };
    (CommunicationControl) => {
        [$crate::UdsServiceType::CommunicationControl]
    };
    (WriteDataByIdentifier) => {
        [$crate::UdsServiceType::WriteDataByIdentifier]
    };
    (RoutineControl) => {
        [$crate::UdsServiceType::RoutineControl]
    };
    (DataTransfer) => {
        [
            $crate::UdsServiceType::RequestDownload,
            $crate::UdsServiceType::TransferData,
            $crate::UdsServiceType::RequestTransferExit,
        ]
    };
    (TesterPresent) => {
        [$crate::UdsServiceType::TesterPresent]
    };
    (ControlDtcSetting) => {
        [$crate::UdsServiceType::ControlDtcSetting]
    };
}

/// What one service contributes to the in-flight buffer's length.
///
/// `#[rustfmt::skip]`: rustfmt does not converge on the two-level associated-type chains
/// below (`<<$ty as _>::Did as _>::MAX_RECORD_LEN`), so the arms are hand-formatted.
#[doc(hidden)]
#[macro_export]
#[rustfmt::skip]
macro_rules! __uds_request_bound {
    // `RequestDownload` is at most 19 bytes: SID, DFI, ALFID, an 8-byte address and an
    // 8-byte size, the widest `uds_protocol` decodes.
    ($ty:ty, DataTransfer) => {
        $crate::assembly::max_of(&[
            2 + <$ty as $crate::DataTransfer>::MAX_BLOCK_LEN,
            19,
        ])
    };
    ($ty:ty, ReadDataByIdentifier) => {
        1 + 2 * <$ty as $crate::ReadDataByIdentifier>::MAX_DIDS_PER_REQUEST
    };
    ($ty:ty, WriteDataByIdentifier) => {
        3 + <<$ty as $crate::WriteDataByIdentifier>::Did
            as $crate::DataIdentifier>::MAX_RECORD_LEN
    };
    ($ty:ty, RoutineControl) => {
        4 + <$ty as $crate::RoutineControl>::MAX_OPTION_LEN
    };
    ($ty:ty, SecurityAccess) => {
        2 + $crate::assembly::max_of(&[
            <$ty as $crate::SecurityAccess>::MAX_KEY_LEN,
            <$ty as $crate::SecurityAccess>::MAX_RECORD_LEN,
        ])
    };
    ($ty:ty, ControlDtcSetting) => {
        2 + <$ty as $crate::ControlDtcSetting>::MAX_OPTION_RECORD_LEN
    };
    // Clause 12.3's widest request is 0x18/0x19: service identifier, sub-function, a
    // three-byte DTC, a record number and a memorySelection byte. Over the catch-all.
    ($ty:ty, ReadDtcInformation) => { 7_usize };
    // Every other request is a service identifier, a sub-function and at most four
    // parameter bytes. Clause-fixed, so a constant.
    ($ty:ty, $svc:ident) => { 6_usize };
}

/// What one service contributes to the response buffer's length.
///
/// `#[rustfmt::skip]`: as `__uds_request_bound!`, for the `ReadDataByIdentifier` and
/// `RoutineControl` arms.
#[doc(hidden)]
#[macro_export]
#[rustfmt::skip]
macro_rules! __uds_response_bound {
    ($ty:ty, ReadDataByIdentifier) => {
        1 + <$ty as $crate::ReadDataByIdentifier>::MAX_DIDS_PER_REQUEST
            * (2 + <<$ty as $crate::ReadDataByIdentifier>::Did
                as $crate::DataIdentifier>::MAX_RECORD_LEN)
    };
    ($ty:ty, ReadDtcInformation) => {
        $crate::assembly::dtc_response_bound(
            <$ty as $crate::ReadDtcInformation>::REPORTS,
            <$ty as $crate::ReadDtcInformation>::MAX_DTCS,
            0,
        )
    };
    // `76 <counter>` or `77`, and a short `transferResponseParameterRecord`.
    ($ty:ty, DataTransfer) => {
        6_usize
    };
    // The service identifier, the echoed type, the identifier and `routineInfo`.
    ($ty:ty, RoutineControl) => {
        5 + <<$ty as $crate::RoutineControl>::Rid
            as $crate::RoutineIdentifier>::MAX_STATUS_LEN
    };
    ($ty:ty, SecurityAccess) => {
        2 + <$ty as $crate::SecurityAccess>::MAX_SEED_LEN
    };
    ($ty:ty, DiagnosticSessionControl) => {
        6 + <$ty as $crate::DiagnosticSessionControl>::MAX_RESPONSE_LEN
    };
    // Everything else answers with a service identifier, an echoed sub-function and at
    // most four bytes. A negative response is three and never larger.
    ($ty:ty, $svc:ident) => { 6_usize };
}

/// Whether one service permits `requestCorrectlyReceivedResponsePending`.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_may_pend {
    // TesterPresent cannot be in progress when a deadline passes (see its trait docs), so
    // it declares no constant.
    ($ty:ty, TesterPresent) => {
        false
    };
    ($ty:ty, $svc:ident) => {
        <$ty as $crate::$svc>::MAY_RESPOND_PENDING
    };
}

/// Emits the body of the two session hooks: a call to `on_transition` where the
/// assembly lists `DiagnosticSessionControl`, nothing otherwise. Used once per listed
/// service, so the call appears at most once.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_session_hook {
    ($self:ident, $entered:expr, DiagnosticSessionControl) => {
        <<Self as $crate::ServiceSet>::App as $crate::DiagnosticSessionControl>::on_transition(
            $self,
            $entered.transition,
            $entered.session,
            $entered.security_relocked,
        );
    };
    ($self:ident, $entered:expr, $svc:ident) => {};
}

/// `ServiceSet::reset_confirmed`'s call into the application, for one listed service:
/// only `EcuReset` has one.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_reset_hook {
    ($self:ident, $kind:ident, EcuReset) => {
        <<Self as $crate::ServiceSet>::App as $crate::EcuReset>::confirmed($self, $kind);
    };
    ($self:ident, $kind:ident, $svc:ident) => {};
}

/// The `P2` pair of the session in force, for one listed service: only
/// `DiagnosticSessionControl` states one, and any other listed service emits nothing.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_session_timing {
    ($self:ident, $state:ident, DiagnosticSessionControl) => {
        return ::core::option::Option::Some($crate::pipeline::session_timing(
            $self, $state,
        ));
    };
    ($self:ident, $state:ident, $svc:ident) => {};
}

/// `ServiceSet::leaves_running_software` for one listed service: only
/// `DiagnosticSessionControl` changes session, and any other listed service emits nothing.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_leaves {
    ($self:ident, $session:ident, DiagnosticSessionControl) => {
        return <<Self as $crate::ServiceSet>::App as $crate::DiagnosticSessionControl>::leaves_running_software(
            $self, $session,
        );
    };
    ($self:ident, $session:ident, $svc:ident) => {};
}

/// Annex I transition 1 for one listed service, from `ServiceSet::start_up`: only
/// `SecurityAccess` owes anything at start-up.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_start_up {
    ($self:ident, SecurityAccess) => {
        $crate::pipeline::security_start_up($self);
    };
    ($self:ident, $svc:ident) => {};
}

/// Figure 6's sub-function lookup for one listed service (``UDSSVC_ARCH_0007``): an early
/// `return` of the [`Access`](crate::Access) the sub-function `$value` has, from the
/// closure `pipeline::begin` is handed, where `$service` is this one. `begin` settles
/// 0x12, 0x7E and 0x33 from it, in Figure 6's order. Only a service with a stage decides
/// its sub-function; any other listed service emits nothing, and the closure's
/// fall-through, available everywhere, passes the request on to the decode and to the
/// 0x11 its missing stage settles.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_sub_function_access {
    (
        $self:ident, $service:ident, $value:ident, $active:ident, DiagnosticSessionControl
    ) => {
        if ::core::matches!($service, $crate::UdsServiceType::DiagnosticSessionControl) {
            return $crate::pipeline::session_access($self, $value, $active);
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, TesterPresent) => {
        if ::core::matches!($service, $crate::UdsServiceType::TesterPresent) {
            return $crate::pipeline::zero_sub_function($value);
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, SecurityAccess) => {
        if ::core::matches!($service, $crate::UdsServiceType::SecurityAccess) {
            return $crate::pipeline::security_access_sessions($self, $value);
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, EcuReset) => {
        if ::core::matches!($service, $crate::UdsServiceType::EcuReset) {
            return $crate::pipeline::sub_function_access($value, |kind| {
                <<Self as $crate::ServiceSet>::App as $crate::EcuReset>::access($self, kind)
            });
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, CommunicationControl) => {
        if ::core::matches!($service, $crate::UdsServiceType::CommunicationControl) {
            return $crate::pipeline::sub_function_access($value, |kind| {
                <<Self as $crate::ServiceSet>::App as $crate::CommunicationControl>::access(
                    $self, kind,
                )
            });
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, ControlDtcSetting) => {
        if ::core::matches!($service, $crate::UdsServiceType::ControlDtcSetting) {
            return $crate::pipeline::dtc_setting_access($value, |setting| {
                <<Self as $crate::ServiceSet>::App as $crate::ControlDtcSetting>::access(
                    $self, setting,
                )
            });
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, ReadDtcInformation) => {
        if ::core::matches!($service, $crate::UdsServiceType::ReadDtcInfo) {
            return $crate::pipeline::sub_function_access($value, |report| {
                <<Self as $crate::ServiceSet>::App as $crate::ReadDtcInformation>::access(
                    $self, report,
                )
            });
        }
    };
    ($self:ident, $service:ident, $value:ident, $active:ident, $svc:ident) => {};
}

/// The checks one listed service asks of the raw request between Table 23 and its decode,
/// for `pipeline::begin_with`'s `ahead`: only `DataTransfer` has any.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_ahead_of_decode {
    ($self:ident, $service:ident, $active:ident, $bytes:ident, DataTransfer) => {
        if let ::core::option::Option::Some(nrc) =
            $crate::pipeline::transfer_ahead_of_decode($self, $service, $active, $bytes)
        {
            return ::core::option::Option::Some(nrc);
        }
    };
    ($self:ident, $service:ident, $active:ident, $bytes:ident, $svc:ident) => {};
}

/// The checks a listed service's stage makes ahead of the request's total length, for a
/// request too long to have been received whole: an early `return` of the first one that
/// fails, from the closure the truncated branch of `dispatch` calls. Only a service whose
/// stage checks something before the length emits one: an identifier in its
/// data-parameters (WDBI, RoutineControl) or the request sequence (SecurityAccess's
/// and the transfer services' 0x24, `TransferData`'s 0x73 and 0x31); for the others
/// `begin` has already asked every such question, and the request is 0x13.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_before_length {
    ($self:ident, $state:ident, $req:ident, WriteDataByIdentifier) => {
        if let $crate::Request::WriteDataByIdentifier(ref r) = $req {
            return $crate::pipeline::writable_identifier($self, $state, r).map(|_| ());
        }
    };
    ($self:ident, $state:ident, $req:ident, RoutineControl) => {
        if let $crate::Request::RoutineControl(ref r) = $req {
            return $crate::pipeline::admitted_routine($self, $state, r).map(|_| ());
        }
    };
    ($self:ident, $state:ident, $req:ident, SecurityAccess) => {
        if let $crate::Request::SecurityAccess(ref r) = $req {
            return $crate::pipeline::key_in_sequence($state, r);
        }
    };
    ($self:ident, $state:ident, $req:ident, DataTransfer) => {
        if ::core::matches!(
            $req,
            $crate::Request::TransferData(_) | $crate::Request::RequestTransferExit(_)
        ) {
            return $crate::pipeline::transfer_before_length($state, &$req);
        }
    };
    ($self:ident, $state:ident, $req:ident, $svc:ident) => {};
}

/// One match arm per listed service, routing its decoded request to its stage. A service
/// with no arm hits the wildcard, which emits nothing; the request then reaches the
/// fall-through after every arm in `dispatch`, an outcome of 0x11 that the driver settles.
/// `RequestUpload` and `RequestFileTransfer` are such services: `DataTransfer`'s list does
/// not name them. A stage whose handler is `async` is awaited;
/// the others are plain calls.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_stage {
    ($self:ident, $state:ident, $out:ident, $req:ident, ReadDataByIdentifier) => {
        if let $crate::Request::ReadDataByIdentifier(ref r) = $req {
            return $crate::pipeline::read_data_by_identifier($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, DiagnosticSessionControl) => {
        if let $crate::Request::DiagnosticSessionControl(ref r) = $req {
            return $crate::pipeline::diagnostic_session_control($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, TesterPresent) => {
        if let $crate::Request::TesterPresent(_) = $req {
            return $crate::pipeline::tester_present($self, $out).map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, EcuReset) => {
        if let $crate::Request::EcuReset(ref r) = $req {
            return $crate::pipeline::ecu_reset($self, r, $out).await;
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, SecurityAccess) => {
        if let $crate::Request::SecurityAccess(ref r) = $req {
            return $crate::pipeline::security_access($self, $state, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, ControlDtcSetting) => {
        if let $crate::Request::ControlDtcSetting(ref r) = $req {
            return $crate::pipeline::control_dtc_setting($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, CommunicationControl) => {
        if let $crate::Request::CommunicationControl(ref r) = $req {
            return $crate::pipeline::communication_control($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, WriteDataByIdentifier) => {
        if let $crate::Request::WriteDataByIdentifier(ref r) = $req {
            return $crate::pipeline::write_data_by_identifier($self, $state, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, ClearDiagnosticInformation) => {
        if let $crate::Request::ClearDiagnosticInfo(ref r) = $req {
            return $crate::pipeline::clear_diagnostic_information($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, ReadDtcInformation) => {
        if let $crate::Request::ReadDtcInfo(ref r) = $req {
            return $crate::pipeline::read_dtc_information($self, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, RoutineControl) => {
        if let $crate::Request::RoutineControl(ref r) = $req {
            return $crate::pipeline::routine_control($self, $state, r, $out)
                .await
                .map($crate::Effect::from);
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, DataTransfer) => {
        if let $crate::Request::RequestDownload(ref r) = $req {
            return $crate::pipeline::request_download($self, $state, r, $out).await;
        }
        if let $crate::Request::TransferData(ref r) = $req {
            return $crate::pipeline::transfer_data($self, $state, r, $out).await;
        }
        if let $crate::Request::RequestTransferExit(ref r) = $req {
            return $crate::pipeline::request_transfer_exit($self, $state, r, $out).await;
        }
    };
    ($self:ident, $state:ident, $out:ident, $req:ident, $svc:ident) => {};
}

/// The compile-time checks one listed service adds to the assembly: `DataTransfer`'s
/// advertised block must reach the transport whole (15.2.3.2), and its
/// `maxNumberOfBlockLength` must fit the four bytes a download-only response holds;
/// `ReadDtcInformation` must list only layouts its `MAX_DTCS` bounds.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_assert {
    ($ty:ty, $transport:ty, ReadDtcInformation) => {
        const _: () = ::core::assert!(
            !$crate::assembly::bounded_in_bytes(
                <$ty as $crate::ReadDtcInformation>::REPORTS
            ),
            "REPORTS lists DtcReportKind::SnapshotRecords or ExtDataRecords, which a \
             server cannot answer: MAX_DTCS does not bound them",
        );
    };
    ($ty:ty, $transport:ty, DataTransfer) => {
        const _: () = ::core::assert!(
            <$transport as $crate::UdsTransport>::MAX_PDU
                >= 2 + <$ty as $crate::DataTransfer>::MAX_BLOCK_LEN,
            "the transport's MAX_PDU cannot carry a TransferData block of \
             MAX_BLOCK_LEN, which ISO 14229-1:2020 15.2.3.2 obliges the server to accept",
        );
        const _: () = ::core::assert!(
            <$ty as $crate::DataTransfer>::MAX_BLOCK_LEN >= 1,
            "MAX_BLOCK_LEN is zero, so no TransferData block could carry data",
        );
        const _: () = ::core::assert!(
            <$ty as $crate::DataTransfer>::MAX_BLOCK_LEN <= 0xFFFF_FFFD,
            "maxNumberOfBlockLength (MAX_BLOCK_LEN + 2) must fit in four bytes",
        );
    };
    ($ty:ty, $transport:ty, $svc:ident) => {};
}

/// Assemble a server from the services it implements.
///
/// ``UDSSVC_ARCH_0013``, ``UDSSVC_ARCH_0035``.
///
/// `transport = T` lets the transport's [`MAX_PDU`](crate::UdsTransport::MAX_PDU) join
/// the fold; the *application* names the type, so ``UDSSVC_ARCH_0002`` and ``0003`` hold.
/// `peers = N` sizes the association array of the `uds_session::Server<N>` the driver
/// owns. `services = Assembly` names the marker type the macro declares and implements
/// this crate's sealed [`ServiceSet`](crate::ServiceSet) on, with the application as its
/// [`App`](crate::ServiceSet::App). `server = Name` is the alias the server is reached
/// through: the macro emits `type Name = Server<Assembly, Transport, N>`. Both are private
/// to the invoking module. The application type may come from any crate, so one
/// application shared by firmware, a simulator and tests is assembled in each.
///
/// `peers = 1` is the only count that compiles: the driver keeps one slot for a
/// `DiagnosticSessionControl` or `ECUReset` response awaiting its confirmation, and a
/// second tester's selecting response could overwrite it while the first is still
/// unconfirmed.
///
/// The assembly also fails to compile where the response buffer, or the transport's
/// [`MAX_PDU`](crate::UdsTransport::MAX_PDU), is shorter than
/// [`ResponseSink::MIN_BOUND`](crate::ResponseSink::MIN_BOUND), because a negative
/// response would then not fit.
///
/// Where `DataTransfer` is listed, it also fails to compile where `MAX_BLOCK_LEN` is
/// zero, or the transport's [`MAX_PDU`](crate::UdsTransport::MAX_PDU) is shorter than
/// `MAX_BLOCK_LEN + 2`, the `TransferData` request length the server advertises and
/// must accept (ISO 14229-1:2020 15.2.3.2).
///
/// Where `ReadDtcInformation` is listed, it also fails to compile where
/// [`REPORTS`](crate::ReadDtcInformation::REPORTS) names a layout whose
/// [`record_len`](crate::DtcReportKind::record_len) is `None`, which
/// [`MAX_DTCS`](crate::ReadDtcInformation::MAX_DTCS) does not bound.
///
/// ```compile_fail,E0080
/// # use uds_services::{
/// #     Access, Address, AfterSend, Ai, DataFormatIdentifier, DataIdentifier, DataTransfer,
/// #     ReadDataByIdentifier, RecordError, Reloads, ResponseSink, ServerParams, ServiceSet,
/// #     Sessions, Storage, Timestamp, TransferRequest, TransportEvent, UdsTransport,
/// #     uds_server,
/// # };
/// # use uds_protocol::NegativeResponseCode as Nrc;
/// use static_cell::ConstStaticCell;
/// #
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Did { VehicleSpeed, VinNumber }
/// #
/// # impl DataIdentifier for Did {
/// #     const MAX_RECORD_LEN: usize = 17;
/// #     fn as_u16(self) -> u16 {
/// #         match self { Self::VehicleSpeed => 0xF4_0D, Self::VinNumber => 0xF1_90 }
/// #     }
/// #     fn from_u16(v: u16) -> Option<Self> {
/// #         match v {
/// #             0xF4_0D => Some(Self::VehicleSpeed),
/// #             0xF1_90 => Some(Self::VinNumber),
/// #             _ => None,
/// #         }
/// #     }
/// #     fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
/// #         let width = match self { Self::VehicleSpeed => 1, Self::VinNumber => 17 };
/// #         buf.split_at_checked(width).ok_or(RecordError::Short)
/// #     }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct Ecu;
/// #
/// # impl Ecu {
/// #     const fn new() -> Self { Self }
/// # }
/// #
/// # impl ReadDataByIdentifier for Ecu {
/// #     type Did = Did;
/// #     const MAY_RESPOND_PENDING: bool = false;
/// #     const MAX_DIDS_PER_REQUEST: usize = 4;
/// #     async fn read(
/// #         &mut self,
/// #         _did: Did,
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// # }
/// #
/// # impl DataTransfer for Ecu {
/// #     const MAY_RESPOND_PENDING: bool = true;
/// #     const MAX_BLOCK_LEN: usize = 1 << 20;
/// #     const SESSIONS: Sessions = Sessions::ALL;
/// #     fn format_supported(&self, _format: DataFormatIdentifier) -> bool { true }
/// #     fn access(&self, _r: &TransferRequest<'_>) -> Option<Access> {
/// #         Some(Access::new(Sessions::ALL))
/// #     }
/// #     async fn begin(&mut self, _r: TransferRequest<'_>) -> Result<(), Nrc> { Ok(()) }
/// #     async fn block(
/// #         &mut self,
/// #         _data: &[u8],
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// #     async fn exit(
/// #         &mut self,
/// #         _record: &[u8],
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct DoIpTransport;
/// #
/// # impl UdsTransport for DoIpTransport {
/// #     type Error = ();
/// #     async fn t_data_req(&mut self, _: Ai, _: &[u8], _: AfterSend) -> Result<(), ()> {
/// #         Ok(())
/// #     }
/// #     async fn next_event<'b>(
/// #         &mut self,
/// #         _buffer: &'b mut [u8],
/// #         _deadline: Option<Timestamp>,
/// #     ) -> Result<TransportEvent<'b>, ()> { Ok(TransportEvent::Deadline) }
/// # const MAX_PDU: usize = 4_096;
/// #     fn outbound_max(&self) -> Option<usize> { None }
/// #     fn channel_timing(&self) -> Reloads {
/// #         Reloads { default_reload: 2_000, enhanced_reload: 5_000 }
/// #     }
/// #     fn now(&self) -> Timestamp { Timestamp(0) }
/// # }
/// #
/// # const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 0);
/// uds_server! {
///     Ecu: ReadDataByIdentifier, DataTransfer;
///     transport = DoIpTransport,
///     peers = 1,
///     services = EcuServices,
///     server = EcuServer,
/// }
///
/// // Constructed in place: no stack temporary holds the buffers on the way in. The cell
/// // hands out the `&'static mut` that `step` needs, once.
/// static SERVER: ConstStaticCell<EcuServer> = ConstStaticCell::new(EcuServer::new(
///     Ecu::new(),
///     DoIpTransport,
///     Address(0x0E00),
///     PARAMS,
/// ));
///
/// let server: &'static mut EcuServer = SERVER.take();
/// // One step, polled by hand here; an executor's task would `server.run().await`.
/// let mut step = core::pin::pin!(server.step());
/// let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
/// assert!(step.as_mut().poll(&mut cx).is_ready());
///
/// let mut store = <<EcuServices as ServiceSet>::Store as Storage>::EMPTY;
/// let buffers = store.split();
/// assert_eq!(buffers.in_flight.len(), 1_026);
/// assert_eq!(buffers.response.len(), 77);
/// ```
///
/// The emitted `dispatch` only routes: `pipeline::begin`, then the listed service's
/// stage, and it returns what they left as an [`Unsettled`](crate::Unsettled). The driver
/// calls `pipeline::settle` once the handler has finished (``UDSSVC_ARCH_0016``). Every
/// clause 8.7 decision is the pipeline's.
///
/// Figure 6's sub-function questions are answered by one lookup per service trait, an
/// [`Access`](crate::Access) for a sub-function the service supports (``UDSSVC_ARCH_0007``
/// rows 2 and 4, then Figure 6's optional 0x33), from which the pipeline decides 0x12,
/// 0x7E or 0x33. The lookup is `access` on [`EcuReset`] and on every other staged service
/// with a sub-function, except three: `DiagnosticSessionControl` answers with
/// [`supports`] and [`supported_from`], `SecurityAccess` with [`sessions`], and
/// `TesterPresent` with nothing, its only sub-function being available everywhere.
/// `RoutineControl` is not asked here: Figure 5 keeps 0x31 out of the sub-function stage,
/// so its `access` is keyed by routine and asked by its own stage.
///
/// [`supports`]: crate::DiagnosticSessionControl::supports
/// [`supported_from`]: crate::DiagnosticSessionControl::supported_from
/// [`sessions`]: crate::SecurityAccess::sessions
/// [`EcuReset`]: crate::EcuReset
///
/// # Examples
///
/// Two services, a transport, and the buffer lengths the macro folds from what they
/// declared. Nothing here picks a size: `1_026` is [`MAX_BLOCK_LEN`] plus its service
/// identifier and block sequence counter, and `77` is [`MAX_DIDS_PER_REQUEST`] records of
/// [`MAX_RECORD_LEN`] plus their identifiers.
///
/// [`MAX_BLOCK_LEN`]: crate::DataTransfer::MAX_BLOCK_LEN
/// [`MAX_DIDS_PER_REQUEST`]: crate::ReadDataByIdentifier::MAX_DIDS_PER_REQUEST
/// [`MAX_RECORD_LEN`]: crate::DataIdentifier::MAX_RECORD_LEN
///
/// ```
/// # use uds_services::{
/// #     Access, Address, AfterSend, Ai, DataFormatIdentifier, DataIdentifier, DataTransfer,
/// #     ReadDataByIdentifier, RecordError, Reloads, ResponseSink, ServerParams, ServiceSet,
/// #     Sessions, Storage, Timestamp, TransferRequest, TransportEvent, UdsTransport,
/// #     uds_server,
/// # };
/// # use uds_protocol::NegativeResponseCode as Nrc;
/// use static_cell::ConstStaticCell;
/// #
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Did { VehicleSpeed, VinNumber }
/// #
/// # impl DataIdentifier for Did {
/// #     const MAX_RECORD_LEN: usize = 17;
/// #     fn as_u16(self) -> u16 {
/// #         match self { Self::VehicleSpeed => 0xF4_0D, Self::VinNumber => 0xF1_90 }
/// #     }
/// #     fn from_u16(v: u16) -> Option<Self> {
/// #         match v {
/// #             0xF4_0D => Some(Self::VehicleSpeed),
/// #             0xF1_90 => Some(Self::VinNumber),
/// #             _ => None,
/// #         }
/// #     }
/// #     fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
/// #         let width = match self { Self::VehicleSpeed => 1, Self::VinNumber => 17 };
/// #         buf.split_at_checked(width).ok_or(RecordError::Short)
/// #     }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct Ecu;
/// #
/// # impl Ecu {
/// #     const fn new() -> Self { Self }
/// # }
/// #
/// # impl ReadDataByIdentifier for Ecu {
/// #     type Did = Did;
/// #     const MAY_RESPOND_PENDING: bool = false;
/// #     const MAX_DIDS_PER_REQUEST: usize = 4;
/// #     async fn read(
/// #         &mut self,
/// #         _did: Did,
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// # }
/// #
/// # impl DataTransfer for Ecu {
/// #     const MAY_RESPOND_PENDING: bool = true;
/// #     const MAX_BLOCK_LEN: usize = 1_024;
/// #     const SESSIONS: Sessions = Sessions::ALL;
/// #     fn format_supported(&self, _format: DataFormatIdentifier) -> bool { true }
/// #     fn access(&self, _r: &TransferRequest<'_>) -> Option<Access> {
/// #         Some(Access::new(Sessions::ALL))
/// #     }
/// #     async fn begin(&mut self, _r: TransferRequest<'_>) -> Result<(), Nrc> { Ok(()) }
/// #     async fn block(
/// #         &mut self,
/// #         _data: &[u8],
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// #     async fn exit(
/// #         &mut self,
/// #         _record: &[u8],
/// #         _out: &mut ResponseSink<'_>,
/// #     ) -> Result<(), Nrc> { Ok(()) }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct DoIpTransport;
/// #
/// # impl UdsTransport for DoIpTransport {
/// #     type Error = ();
/// #     async fn t_data_req(&mut self, _: Ai, _: &[u8], _: AfterSend) -> Result<(), ()> {
/// #         Ok(())
/// #     }
/// #     async fn next_event<'b>(
/// #         &mut self,
/// #         _buffer: &'b mut [u8],
/// #         _deadline: Option<Timestamp>,
/// #     ) -> Result<TransportEvent<'b>, ()> { Ok(TransportEvent::Deadline) }
/// #     fn outbound_max(&self) -> Option<usize> { None }
/// #     fn channel_timing(&self) -> Reloads {
/// #         Reloads { default_reload: 2_000, enhanced_reload: 5_000 }
/// #     }
/// #     fn now(&self) -> Timestamp { Timestamp(0) }
/// # }
/// #
/// # const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 0);
/// uds_server! {
///     Ecu: ReadDataByIdentifier, DataTransfer;
///     transport = DoIpTransport,
///     peers = 1,
///     services = EcuServices,
///     server = EcuServer,
/// }
///
/// // Constructed in place: no stack temporary holds the buffers on the way in. The cell
/// // hands out the `&'static mut` that `step` needs, once.
/// static SERVER: ConstStaticCell<EcuServer> = ConstStaticCell::new(EcuServer::new(
///     Ecu::new(),
///     DoIpTransport,
///     Address(0x0E00),
///     PARAMS,
/// ));
///
/// let server: &'static mut EcuServer = SERVER.take();
/// // One step, polled by hand here; an executor's task would `server.run().await`.
/// let mut step = core::pin::pin!(server.step());
/// let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
/// assert!(step.as_mut().poll(&mut cx).is_ready());
///
/// let mut store = <<EcuServices as ServiceSet>::Store as Storage>::EMPTY;
/// let buffers = store.split();
/// assert_eq!(buffers.in_flight.len(), 1_026);
/// assert_eq!(buffers.response.len(), 77);
/// ```
#[macro_export]
macro_rules! uds_server {
    (
        $ty:ty : $($svc:ident),+ $(,)? ;
        transport = $transport:ty,
        peers = $peers:expr,
        services = $services:ident,
        server = $server:ident $(,)?
    ) => {
        /// The assembled services of this application, on which `uds_server!`
        /// implements `ServiceSet`. Never a value: it names the assembly in types.
        #[derive(Debug)]
        enum $services {}

        /// The assembled server: this application's services, storage, session layer and
        /// transport. Emitted by `uds_server!`.
        type $server = $crate::Server<$services, $transport, { $peers }>;

        const _: () = {
            const IN_FLIGHT: usize = $crate::assembly::min2(
                $crate::assembly::max_of(&[
                    $( $crate::__uds_request_bound!($ty, $svc) ),+
                ]),
                <$transport as $crate::UdsTransport>::MAX_PDU,
            );
            const RESPONSE: usize = $crate::assembly::min2(
                $crate::assembly::max_of(&[
                    $( $crate::__uds_response_bound!($ty, $svc) ),+
                ]),
                <$transport as $crate::UdsTransport>::MAX_PDU,
            );
            // The only message clause 8.7.6 admits while a service is in progress is a
            // two-byte TesterPresent; anything larger is answered busyRepeatRequest from
            // its service identifier alone, so this buffer is wide enough to read that
            // identifier.
            const CONCURRENT: usize = 8;
            // A negative response fits only if the buffer under the sink holds one:
            // `ResponseSink::new` raises its bound to the floor but not past the buffer.
            const _: () = ::core::assert!(
                <$transport as $crate::UdsTransport>::MAX_PDU
                    >= $crate::ResponseSink::MIN_BOUND,
                "the transport's MAX_PDU cannot carry a negative response",
            );
            const _: () = ::core::assert!(
                RESPONSE >= $crate::ResponseSink::MIN_BOUND,
                "the response buffer cannot hold a negative response",
            );
            $( $crate::__uds_assert!($ty, $transport, $svc); )+
            // One slot for a selecting response awaiting confirmation, so a second peer
            // could overwrite it; zero peers can never answer.
            const _: () = ::core::assert!(
                { $peers } == 1,
                "milestone 1 supports `peers = 1` only",
            );

            impl $crate::sealed::Sealed for $services {}

            impl $crate::ServiceSet for $services {
                type App = $ty;

                type Store = $crate::Store<IN_FLIGHT, CONCURRENT, RESPONSE>;

                type State = $crate::State;

                async fn dispatch(
                    app: &mut $ty,
                    state: &mut Self::State,
                    // The driver settles, and it is `settle` that reads the addressing.
                    _ai: $crate::Ai,
                    received: $crate::Received<'_>,
                    out: &mut $crate::ResponseSink<'_>,
                ) -> $crate::Unsettled {
                    // Annex I transitions 9 and 10: a refused SecurityAccess request
                    // discards the seed, whichever of the returns below refused it.
                    let unsettled = async {
                        // Unused in an assembly listing no service that decides a
                        // sub-function, where every arm of the helper expands to nothing.
                        #[allow(unused_variables, reason = "used only by some assemblies")]
                        let access = |service: $crate::UdsServiceType,
                                      value: u8,
                                      active: $crate::DiagnosticSessionType| {
                            $( $crate::__uds_sub_function_access!(
                                app, service, value, active, $svc
                            ); )+
                            ::core::option::Option::Some($crate::Access::new(
                                $crate::Sessions::ALL,
                            ))
                        };
                        #[allow(unused_variables, reason = "used only by some assemblies")]
                        let ahead = |service: $crate::UdsServiceType,
                                     active: $crate::DiagnosticSessionType,
                                     bytes: &[u8]|
                         -> ::core::option::Option<$crate::NegativeResponseCode> {
                            $( $crate::__uds_ahead_of_decode!(
                                app, service, active, bytes, $svc
                            ); )+
                            ::core::option::Option::None
                        };
                        let (sid, decoded) = match $crate::pipeline::begin_with(
                            state,
                            received.bytes(),
                            <Self as $crate::ServiceSet>::supports,
                            access,
                            ahead,
                        ) {
                            $crate::pipeline::Stage::Empty => {
                                return $crate::Unsettled::empty();
                            }
                            $crate::pipeline::Stage::Settle { sid, nrc } => {
                                return $crate::Unsettled::refused(sid, nrc);
                            }
                            $crate::pipeline::Stage::Proceed { sid, request }
                                if ::core::matches!(received, $crate::Received::Truncated(_)) =>
                            {
                                let before_length = || -> ::core::result::Result<
                                    (),
                                    $crate::NegativeResponseCode,
                                > {
                                    $( $crate::__uds_before_length!(
                                        app, state, request, $svc
                                    ); )+
                                    ::core::result::Result::Ok(())
                                };
                                let nrc = before_length().err().unwrap_or(
                                    $crate::NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
                                );
                                return $crate::Unsettled::refused(sid, nrc);
                            }
                            $crate::pipeline::Stage::Proceed { sid, request } => (sid, request),
                        };
                        let settling = $crate::pipeline::Settling {
                            sid,
                            suppress_bit: decoded
                                .is_positive_response_suppressed()
                                .unwrap_or(false),
                        };
                        let outcome = async {
                            $( $crate::__uds_stage!(app, state, out, decoded, $svc); )+
                            ::core::result::Result::Err(
                                $crate::NegativeResponseCode::ServiceNotSupported,
                            )
                        }
                        .await;
                        $crate::Unsettled::handled(settling, outcome)
                    }
                    .await;
                    $crate::pipeline::discarding_seed(state, out, unsettled)
                }

                #[allow(
                    unreachable_code,
                    unused_variables,
                    reason = "only an assembly with DiagnosticSessionControl reads it, \
                              and returns before the fall-through"
                )]
                fn leaves_running_software(
                    app: &$ty,
                    session: $crate::DiagnosticSessionType,
                ) -> bool {
                    $( $crate::__uds_leaves!(app, session, $svc); )+
                    false
                }

                #[allow(unused_variables, reason = "only an assembly with SecurityAccess reads it")]
                fn start_up(app: &mut $ty, _state: &mut Self::State) {
                    $( $crate::__uds_start_up!(app, $svc); )+
                }

                #[allow(
                    unused_variables,
                    reason = "only an assembly with DiagnosticSessionControl reads it"
                )]
                fn session_timed_out(app: &mut $ty, state: &mut Self::State) {
                    let entered = $crate::pipeline::transition(
                        state,
                        $crate::DiagnosticSessionType::DefaultSession,
                    );
                    $( $crate::__uds_session_hook!(app, entered, $svc); )+
                }

                #[allow(
                    unused_variables,
                    reason = "only an assembly with DiagnosticSessionControl reads it"
                )]
                fn session_confirmed(
                    app: &mut $ty,
                    state: &mut Self::State,
                    selected: $crate::DiagnosticSessionType,
                ) {
                    let entered = $crate::pipeline::transition(state, selected);
                    $( $crate::__uds_session_hook!(app, entered, $svc); )+
                }

                #[allow(unused_variables, reason = "only an assembly with EcuReset reads it")]
                fn reset_confirmed(
                    app: &mut $ty,
                    state: &mut Self::State,
                    kind: $crate::ResetType,
                    after: $crate::AfterReset,
                ) {
                    $( $crate::__uds_reset_hook!(app, kind, $svc); )+
                    if ::core::matches!(after, $crate::AfterReset::Restart) {
                        if let ::core::option::Option::Some(entered) =
                            $crate::pipeline::restart(state)
                        {
                            $( $crate::__uds_session_hook!(app, entered, $svc); )+
                        }
                    }
                }

                #[allow(
                    unreachable_code,
                    unused_variables,
                    reason = "the arm emitted for DiagnosticSessionControl reads both and \
                              returns"
                )]
                fn session_timing(
                    app: &$ty,
                    state: &Self::State,
                ) -> ::core::option::Option<$crate::SessionTiming> {
                    $( $crate::__uds_session_timing!(app, state, $svc); )+
                    ::core::option::Option::None
                }

                fn supports(service: $crate::UdsServiceType) -> bool {
                    $( if $crate::__uds_sids!($svc).contains(&service) {
                        return true;
                    } )+
                    false
                }

                fn may_respond_pending(service: $crate::UdsServiceType) -> bool {
                    $( if $crate::__uds_sids!($svc).contains(&service) {
                        return $crate::__uds_may_pend!($ty, $svc);
                    } )+
                    false
                }

                fn is_concurrent_exception(request: &[u8], ai: $crate::Ai) -> bool {
                    ::core::matches!(ai.ta_type, $crate::TaType::Functional)
                        && request == [0x3E, 0x80]
                }
            }
        };
    };
}

/// Assemble a client from the identifier vocabulary it speaks.
///
/// ``UDSSVC_ARCH_0020``, ``UDSSVC_ARCH_0024``: the same enumeration a server's handlers
/// are written against names a client's requests, so a vehicle program that builds both
/// from one definition gets a compile error when only one was updated.
///
/// `vocabulary = Name` names the marker type the macro declares and implements this
/// crate's sealed [`ClientSet`](crate::ClientSet) on: the leading type is its
/// [`Did`](crate::ClientSet::Did) and `routines` its
/// [`Routine`](crate::ClientSet::Routine). The identifier types may come from any crate,
/// and one identifier type may take any number of invocations, over different
/// transports or keep-alive modes.
///
/// `limits` is a [`ClientLimits`](crate::ClientLimits), in a `const`: the client's
/// buffers are folded from it and the identifiers' declared lengths as
/// [`crate::uds_server`] folds a server's from its service traits. The request buffer
/// holds the widest request among the client's services, and the response buffer the
/// widest response.
///
/// Naming the transport lets its [`MAX_PDU`](crate::UdsTransport::MAX_PDU) join the fold.
/// A transport whose `MAX_PDU` cannot carry a request of the declared limits fails to
/// compile, naming the limit it cannot carry, as does one with no room for a six-byte
/// `DiagnosticSessionControl` response ([`crate::Client::diagnostic_session_control`]);
/// six bytes also hold every negative response. The response buffer is capped at
/// `MAX_PDU`, since no response longer than that can arrive.
///
/// `client = Name` is the alias the client is reached through, so the channel counts and
/// the keep-alive mode are written once, here, rather than at every signature that names
/// a client. The marker and the alias are private to the invoking module.
///
/// # Examples
///
/// ```
/// # use uds_services::{
/// #     Address, AfterSend, Ai, ClientLimits, ClientTiming, ClientTransport, DataIdentifier,
/// #     KeepAlive, PhysicalKeepAlive, RecordError, Reloads, RoutineIdentifier, Timestamp,
/// #     TransportEvent, UdsTransport, uds_client,
/// # };
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Did { VehicleSpeed, VinNumber }
/// #
/// # impl DataIdentifier for Did {
/// #     const MAX_RECORD_LEN: usize = 17;
/// #     fn as_u16(self) -> u16 {
/// #         match self { Self::VehicleSpeed => 0xF4_0D, Self::VinNumber => 0xF1_90 }
/// #     }
/// #     fn from_u16(v: u16) -> Option<Self> {
/// #         match v {
/// #             0xF4_0D => Some(Self::VehicleSpeed),
/// #             0xF1_90 => Some(Self::VinNumber),
/// #             _ => None,
/// #         }
/// #     }
/// #     fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
/// #         let width = match self { Self::VehicleSpeed => 1, Self::VinNumber => 17 };
/// #         buf.split_at_checked(width).ok_or(RecordError::Short)
/// #     }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct DoIpTransport;
/// # impl UdsTransport for DoIpTransport {
/// #     type Error = ();
/// #     async fn t_data_req(&mut self, _: Ai, _: &[u8], _: AfterSend) -> Result<(), ()> {
/// #         Ok(())
/// #     }
/// #     async fn next_event<'b>(
/// #         &mut self,
/// #         _b: &'b mut [u8],
/// #         _d: Option<Timestamp>,
/// #     ) -> Result<TransportEvent<'b>, ()> { Ok(TransportEvent::Deadline) }
/// #     fn outbound_max(&self) -> Option<usize> { None }
/// #     fn channel_timing(&self) -> Reloads {
/// #         Reloads { default_reload: 2_000, enhanced_reload: 5_000 }
/// #     }
/// #     fn now(&self) -> Timestamp { Timestamp(0) }
/// # }
/// # impl ClientTransport for DoIpTransport {
/// #     async fn close(&mut self) -> Result<(), ()> { Ok(()) }
/// # }
/// #
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Rid { SelfTest }
/// #
/// # impl RoutineIdentifier for Rid {
/// #     const MAX_STATUS_LEN: usize = 1;
/// #     fn as_u16(self) -> u16 { 0x02_01 }
/// #     fn from_u16(v: u16) -> Option<Self> { (v == 0x02_01).then_some(Self::SelfTest) }
/// # }
/// uds_client! {
///     Did;
///     vocabulary = TesterVocabulary,
///     transport = DoIpTransport,
///     routines = Rid,
///     limits = ClientLimits::new()
///         .max_dids_per_request(4)
///         .max_dtcs(4)
///         .max_dtc_record_len(16)
///         .max_seed_len(2)
///         .max_key_len(2)
///         .max_option_len(4)
///         .max_block_len(64),
///     physical = 2,
///     functional = 1,
///     responders = 4,
///     keep_alive = PhysicalKeepAlive,
///     client = Tester,
/// }
///
/// static TESTER: Tester = Tester::new(
///     DoIpTransport,
///     Address(0x0E00),
///     KeepAlive::physical(2_000),
///     ClientTiming::new(50),
/// );
/// # let _ = &TESTER;
/// ```
///
/// A 32-byte `MAX_PDU` cannot carry the 66-byte `TransferData` request a `max_block_len`
/// of 64 declares, so this fails to compile:
///
/// ```compile_fail,E0080
/// # use uds_services::{
/// #     Address, AfterSend, Ai, ClientLimits, ClientTiming, ClientTransport, DataIdentifier,
/// #     KeepAlive, PhysicalKeepAlive, RecordError, Reloads, RoutineIdentifier, Timestamp,
/// #     TransportEvent, UdsTransport, uds_client,
/// # };
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Did { VehicleSpeed, VinNumber }
/// #
/// # impl DataIdentifier for Did {
/// #     const MAX_RECORD_LEN: usize = 17;
/// #     fn as_u16(self) -> u16 {
/// #         match self { Self::VehicleSpeed => 0xF4_0D, Self::VinNumber => 0xF1_90 }
/// #     }
/// #     fn from_u16(v: u16) -> Option<Self> {
/// #         match v {
/// #             0xF4_0D => Some(Self::VehicleSpeed),
/// #             0xF1_90 => Some(Self::VinNumber),
/// #             _ => None,
/// #         }
/// #     }
/// #     fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
/// #         let width = match self { Self::VehicleSpeed => 1, Self::VinNumber => 17 };
/// #         buf.split_at_checked(width).ok_or(RecordError::Short)
/// #     }
/// # }
/// #
/// # #[derive(Debug)]
/// # struct DoIpTransport;
/// # impl UdsTransport for DoIpTransport {
/// #     type Error = ();
/// #     const MAX_PDU: usize = 32;
/// #     async fn t_data_req(&mut self, _: Ai, _: &[u8], _: AfterSend) -> Result<(), ()> {
/// #         Ok(())
/// #     }
/// #     async fn next_event<'b>(
/// #         &mut self,
/// #         _b: &'b mut [u8],
/// #         _d: Option<Timestamp>,
/// #     ) -> Result<TransportEvent<'b>, ()> { Ok(TransportEvent::Deadline) }
/// #     fn outbound_max(&self) -> Option<usize> { None }
/// #     fn channel_timing(&self) -> Reloads {
/// #         Reloads { default_reload: 2_000, enhanced_reload: 5_000 }
/// #     }
/// #     fn now(&self) -> Timestamp { Timestamp(0) }
/// # }
/// # impl ClientTransport for DoIpTransport {
/// #     async fn close(&mut self) -> Result<(), ()> { Ok(()) }
/// # }
/// #
/// # #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// # enum Rid { SelfTest }
/// #
/// # impl RoutineIdentifier for Rid {
/// #     const MAX_STATUS_LEN: usize = 1;
/// #     fn as_u16(self) -> u16 { 0x02_01 }
/// #     fn from_u16(v: u16) -> Option<Self> { (v == 0x02_01).then_some(Self::SelfTest) }
/// # }
/// uds_client! {
///     Did;
///     vocabulary = TesterVocabulary,
///     transport = DoIpTransport,
///     routines = Rid,
///     limits = ClientLimits::new()
///         .max_dids_per_request(4)
///         .max_dtcs(4)
///         .max_dtc_record_len(16)
///         .max_seed_len(2)
///         .max_key_len(2)
///         .max_option_len(4)
///         .max_block_len(64),
///     physical = 2,
///     functional = 1,
///     responders = 4,
///     keep_alive = PhysicalKeepAlive,
///     client = Tester,
/// }
/// ```
#[macro_export]
macro_rules! uds_client {
    (
        $did:ty ;
        vocabulary = $vocabulary:ident,
        transport = $transport:ty,
        routines = $rid:ty,
        limits = $limits:expr,
        physical = $phys:expr,
        functional = $func:expr,
        responders = $responders:expr,
        keep_alive = $keep_alive:ty,
        client = $client:ident $(,)?
    ) => {
        /// The vocabulary this client speaks, on which `uds_client!` implements
        /// `ClientSet`. Never a value: it names the assembly in types.
        #[derive(Debug)]
        enum $vocabulary {}

        /// The assembled client: this application's identifiers, its buffers, session
        /// layer and transport. Emitted by `uds_client!`.
        type $client = $crate::Client<
            $vocabulary,
            $transport,
            $keep_alive,
            { $phys },
            { $func },
            { $responders },
        >;

        const _: () = {
            const LIMITS: $crate::ClientLimits = $limits;
            const MAX_PDU: usize = <$transport as $crate::UdsTransport>::MAX_PDU;
            const MAX_RECORD_LEN: usize = <$did as $crate::DataIdentifier>::MAX_RECORD_LEN;
            const _: () =
                $crate::assembly::assert_client_fits(LIMITS, MAX_RECORD_LEN, MAX_PDU);
            const REQUEST: usize =
                $crate::assembly::client_request_bound(LIMITS, MAX_RECORD_LEN);
            const RESPONSE: usize = $crate::assembly::min2(
                $crate::assembly::client_response_bound(
                    LIMITS,
                    MAX_RECORD_LEN,
                    <$rid as $crate::RoutineIdentifier>::MAX_STATUS_LEN,
                ),
                MAX_PDU,
            );
            const _: () = ::core::assert!(
                RESPONSE >= 6,
                "the transport's MAX_PDU cannot carry a DiagnosticSessionControl response"
            );

            impl $crate::sealed::Sealed for $vocabulary {}

            impl $crate::ClientSet for $vocabulary {
                type Did = $did;
                type Routine = $rid;
                type Store = $crate::ClientStore<REQUEST, RESPONSE>;
                const LIMITS: $crate::ClientLimits = LIMITS;
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_in_bytes, client_request_bound, client_response_bound, dtc_response_bound,
        max_of, min2,
    };
    use crate::{ClientLimits, DtcReportKind};

    /// One of everything that is sized, and no DTC records.
    const LEAST: ClientLimits = ClientLimits::new()
        .max_seed_len(1)
        .max_key_len(1)
        .max_option_len(1)
        .max_block_len(1);

    /// The client's bounds are the widest request and the widest response among its
    /// services: here the transfer block wins both.
    #[test]
    fn the_client_bounds_are_the_widest_service() {
        const LIMITS: ClientLimits = ClientLimits::new()
            .max_dids_per_request(2)
            .max_dtcs(3)
            .max_dtc_record_len(20)
            .max_seed_len(2)
            .max_key_len(2)
            .max_option_len(4)
            .max_block_len(64);
        const REQUEST: usize = client_request_bound(LIMITS, 17);
        const RESPONSE: usize = client_response_bound(LIMITS, 17, 1);
        assert_eq!(REQUEST, 2 + 64);
        assert_eq!(RESPONSE, 2 + 64);
        assert_eq!(client_request_bound(LEAST, 1), 19);
        assert_eq!(client_response_bound(LEAST, 1, 1), 17);
    }

    /// The defaults fold to the fixed-size floors, so a client declaring nothing costs
    /// nothing beyond them.
    #[test]
    fn the_default_limits_fold_to_the_fixed_floors() {
        assert_eq!(client_request_bound(ClientLimits::new(), 1), 19);
        assert_eq!(client_response_bound(ClientLimits::new(), 1, 1), 17);
    }

    /// Each request term is the bound when its limit is the large one.
    #[test]
    fn each_request_term_can_set_the_bound() {
        // A read of twenty identifiers.
        assert_eq!(
            client_request_bound(LEAST.max_dids_per_request(20), 1),
            1 + 2 * 20
        );
        // A write of a thirty-byte record.
        assert_eq!(client_request_bound(LEAST, 30), 3 + 30);
        // A twenty-byte key.
        assert_eq!(client_request_bound(LEAST.max_key_len(20), 1), 2 + 20);
        // A routine control with a twenty-byte option record.
        assert_eq!(client_request_bound(LEAST.max_option_len(20), 1), 4 + 20);
    }

    /// Each response term is the bound when its limit is the large one.
    #[test]
    fn each_response_term_can_set_the_bound() {
        // A read of two seventeen-byte records.
        assert_eq!(
            client_response_bound(LEAST.max_dids_per_request(2), 17, 1),
            1 + 2 * (2 + 17)
        );
        // A DTC list of ten: `SeverityList`'s six-byte records are the widest.
        assert_eq!(client_response_bound(LEAST.max_dtcs(10), 1, 1), 3 + 10 * 6);
        // A snapshot or extended data response of thirty record bytes.
        assert_eq!(
            client_response_bound(LEAST.max_dtc_record_len(30), 1, 1),
            6 + 30
        );
        // A thirty-byte seed.
        assert_eq!(client_response_bound(LEAST.max_seed_len(30), 1, 1), 2 + 30);
        // A routine status record of thirty bytes.
        assert_eq!(client_response_bound(LEAST, 1, 30), 5 + 30);
        // A transfer exit with a thirty-byte parameter record.
        assert_eq!(
            client_response_bound(LEAST.max_option_len(30), 1, 1),
            1 + 30
        );
    }

    /// The widest layout wins, and it is not always the one with the widest records: a
    /// `SeverityList` of ten beats a `DtcList` of ten on record width, and `Count` beats
    /// both when the server reports no records at all.
    #[test]
    fn the_widest_declared_layout_sets_the_bound() {
        const LIST: usize = dtc_response_bound(&[DtcReportKind::DtcList], 10, 0);
        const SEVERITY: usize = dtc_response_bound(&[DtcReportKind::SeverityList], 10, 0);
        const BOTH: usize = dtc_response_bound(
            &[DtcReportKind::DtcList, DtcReportKind::SeverityList],
            10,
            0,
        );
        assert_eq!((LIST, SEVERITY, BOTH), (3 + 40, 3 + 60, 3 + 60));
    }

    /// The header is part of the bound, so a layout carrying no records still needs
    /// room, and `WwhObdSeverity`'s five header bytes plus the `0x59` are counted.
    #[test]
    fn a_layout_without_records_still_needs_its_header() {
        const COUNT: usize = dtc_response_bound(&[DtcReportKind::Count], 10, 0);
        const WWH: usize = dtc_response_bound(&[DtcReportKind::WwhObdSeverity], 10, 0);
        assert_eq!((COUNT, WWH), (6, 6 + 50));
    }

    /// The by-DTC-number layouts are bounded in bytes: six of header, then as many
    /// record bytes as the caller declares, whatever the DTC count.
    #[test]
    fn a_by_dtc_number_layout_is_bounded_by_its_record_bytes() {
        const SNAPSHOT: usize =
            dtc_response_bound(&[DtcReportKind::SnapshotRecords], 3, 20);
        const EXT: usize = dtc_response_bound(&[DtcReportKind::ExtDataRecords], 3, 20);
        assert_eq!((SNAPSHOT, EXT), (6 + 20, 6 + 20));
    }

    /// Mixed layouts each take their own maximum: the DTC count bounds the list and the
    /// record bytes bound the snapshot, so neither is read in the other's unit.
    #[test]
    fn mixed_layouts_are_bounded_each_in_its_own_unit() {
        const KINDS: &[DtcReportKind] =
            &[DtcReportKind::DtcList, DtcReportKind::SnapshotRecords];
        const LIST_WINS: usize = dtc_response_bound(KINDS, 10, 20);
        const BYTES_WIN: usize = dtc_response_bound(KINDS, 10, 100);
        assert_eq!((LIST_WINS, BYTES_WIN), (3 + 10 * 4, 6 + 100));
    }

    /// Only the layouts counted in bytes are flagged, which is what `uds_server!` refuses.
    #[test]
    fn only_byte_bounded_layouts_are_flagged() {
        const LISTS: bool = bounded_in_bytes(&[
            DtcReportKind::Count,
            DtcReportKind::DtcList,
            DtcReportKind::FaultDetectionCounters,
            DtcReportKind::SeverityList,
            DtcReportKind::WwhObdSeverity,
        ]);
        const SNAPSHOT: bool =
            bounded_in_bytes(&[DtcReportKind::DtcList, DtcReportKind::SnapshotRecords]);
        const EXT: bool = bounded_in_bytes(&[DtcReportKind::ExtDataRecords]);
        assert_eq!(
            (LISTS, SNAPSHOT, EXT, bounded_in_bytes(&[])),
            (false, true, true, false)
        );
    }

    /// A server declaring no report types folds to zero, as an empty service set does.
    #[test]
    fn no_declared_layout_folds_to_zero() {
        const NONE: usize = dtc_response_bound(&[], 10, 10);
        assert_eq!(NONE, 0);
    }

    /// The fold the macro performs is const-evaluable: this fails to compile otherwise.
    #[test]
    fn the_maximum_is_const_evaluable() {
        const SIZES: [usize; 3] = [7, 1_026, 300];
        const MAX: usize = max_of(&SIZES);
        assert_eq!(MAX, 1_026);
    }

    /// An empty set folds to zero rather than panicking.
    #[test]
    fn an_empty_set_folds_to_zero() {
        const MAX: usize = max_of(&[]);
        assert_eq!(MAX, 0);
    }

    /// A transport that caps the payload lowers the derived size; one that does not
    /// leaves `MAX_PDU` at `usize::MAX` and is ignored.
    #[test]
    fn a_capping_transport_lowers_the_result() {
        const CAPPED: usize = min2(1_026, 512);
        const UNCAPPED: usize = min2(1_026, usize::MAX);
        assert_eq!((CAPPED, UNCAPPED), (512, 1_026));
    }
}
