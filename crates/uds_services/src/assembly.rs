//! Assembly, and the const arithmetic it performs.
//!
//! ``UDSSVC_ARCH_0013`` — a server is assembled by naming its services, so the set is a
//! list this crate can read rather than an inference from which methods were overridden.
//! `serviceNotSupported` (0x11) is decided from a list, and this is that list.
//!
//! The macro does something the rest of the crate cannot. An associated const of a
//! *generic* parameter is not usable as an array length — `[u8; S::MAX_REQUEST]` is
//! rejected without the unstable `generic_const_exprs` — but at this macro's expansion
//! site the types are concrete, so `<Ecu as DataTransfer>::MAX_BLOCK_LENGTH` is an
//! ordinary const and folding a list of them into an array length is stable Rust. That is
//! why assembly is a macro and not a blanket impl, and it is how [`crate::Storage`]'s
//! sizes are derived without the application picking a number.

/// The largest value in `values`, or zero where there are none.
///
/// `#[doc(hidden)]`: macro plumbing. It is `pub` only because [`crate::uds_server`]
/// expands in the application's crate and has to name it there, the same reason
/// `__uds_request_bound!` and its siblings are.
///
/// Walked by splitting rather than by index: a `const fn` cannot use an iterator, and an
/// index plus a counter needs two prose invariants that this needs none of.
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
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. `core::cmp::min` is not `const` on
/// stable, which is why this exists at all.
#[doc(hidden)]
#[must_use]
pub const fn min2(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

/// The response bound for `ReadDTCInformation`: the widest layout this server answers.
///
/// `#[doc(hidden)]`: macro plumbing, as [`max_of`] is. Saturating throughout because this
/// crate denies `arithmetic_side_effects`, and a declared maximum large enough to
/// overflow `usize` is capped by [`min2`] against
/// [`MAX_PDU`](crate::UdsTransport::MAX_PDU) immediately afterwards.
#[doc(hidden)]
#[must_use]
pub const fn dtc_response_bound(
    mut reports: &[crate::DtcReportKind],
    max_dtcs: usize,
) -> usize {
    let mut best = 0;
    while let Some((&kind, rest)) = reports.split_first() {
        let size = kind
            .header_len()
            .saturating_add(max_dtcs.saturating_mul(kind.record_len()));
        if size > best {
            best = size;
        }
        reports = rest;
    }
    best
}

/// Every service identifier a service trait covers.
#[doc(hidden)]
#[macro_export]
macro_rules! __uds_sids {
    (DiagnosticSessionControl) => {
        [0x10_u8]
    };
    (EcuReset) => {
        [0x11_u8]
    };
    (ClearDiagnosticInformation) => {
        [0x14_u8]
    };
    (ReadDtcInformation) => {
        [0x19_u8]
    };
    (ReadDataByIdentifier) => {
        [0x22_u8]
    };
    (SecurityAccess) => {
        [0x27_u8]
    };
    (CommunicationControl) => {
        [0x28_u8]
    };
    (WriteDataByIdentifier) => {
        [0x2E_u8]
    };
    (RoutineControl) => {
        [0x31_u8]
    };
    (DataTransfer) => {
        [0x34_u8, 0x35_u8, 0x36_u8, 0x37_u8, 0x38_u8]
    };
    (TesterPresent) => {
        [0x3E_u8]
    };
    (ControlDtcSetting) => {
        [0x85_u8]
    };
}

/// What one service contributes to the in-flight buffer's length.
///
/// `#[rustfmt::skip]`: rustfmt 1.9.0 does not converge on the two-level associated-type
/// chains below (`<<$ty as _>::Did as _>::MAX_RECORD_LEN`) — each `cargo fmt` run adds
/// further indentation to the continuation line rather than reaching a fixed point. The
/// arms are hand-formatted instead, verified to stay within the crate's 92-column limit.
#[doc(hidden)]
#[macro_export]
#[rustfmt::skip]
macro_rules! __uds_request_bound {
    ($ty:ty, DataTransfer) => {
        2 + <$ty as $crate::DataTransfer>::MAX_BLOCK_LENGTH
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
        2 + <$ty as $crate::SecurityAccess>::MAX_KEY_LEN
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
/// `#[rustfmt::skip]`: see `__uds_request_bound!`'s note above — the same non-convergent
/// chain formatting affects the `ReadDataByIdentifier` and `RoutineControl` arms here.
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
        )
    };
    ($ty:ty, DataTransfer) => {
        if <$ty as $crate::DataTransfer>::SUPPORTS_UPLOAD {
            2 + <$ty as $crate::DataTransfer>::MAX_BLOCK_LENGTH
        } else {
            6_usize
        }
    };
    ($ty:ty, RoutineControl) => {
        4 + <<$ty as $crate::RoutineControl>::Rid
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
    // Neither service can be in progress when a deadline passes — see their trait docs —
    // so neither declares the constant and the answer is not the application's to give.
    ($ty:ty, TesterPresent) => {
        false
    };
    ($ty:ty, DiagnosticSessionControl) => {
        false
    };
    ($ty:ty, $svc:ident) => {
        <$ty as $crate::$svc>::MAY_RESPOND_PENDING
    };
}

/// Assemble a server from the services it implements.
///
/// ``UDSSVC_ARCH_0013``, ``UDSSVC_ARCH_0035``.
///
/// Naming the transport lets its [`MAX_PDU`](crate::UdsTransport::MAX_PDU) join the fold.
/// It introduces no dependency on a binding: the *application* names the type, and it
/// already depends on both crates, so ``UDSSVC_ARCH_0002`` and ``0003`` are untouched.
///
/// `peers = N` sizes the association array of the `uds_session::Server<N>` the driver
/// owns, and `server = Name` is the alias it is reached through — the macro emits
/// `type Name = Server<Ecu, Transport, N>`, so the count is written once, where it acts.
///
/// The syntax is `Ecu: ..; transport = T, ..` rather than `Ecu over T: ..` because
/// `$ty:ty` cannot be followed by a bare identifier — the legal followers are
/// `{ [ => , > = : ; | as where`.
///
/// # Examples
///
/// Two services, a transport, and the buffer lengths the macro folds from what they
/// declared. Nothing here picks a size: `1_026` is [`MAX_BLOCK_LENGTH`] plus its service
/// identifier and block sequence counter, and `77` is [`MAX_DIDS_PER_REQUEST`] records of
/// [`MAX_RECORD_LEN`] plus their identifiers.
///
/// [`MAX_BLOCK_LENGTH`]: crate::DataTransfer::MAX_BLOCK_LENGTH
/// [`MAX_DIDS_PER_REQUEST`]: crate::ReadDataByIdentifier::MAX_DIDS_PER_REQUEST
/// [`MAX_RECORD_LEN`]: crate::DataIdentifier::MAX_RECORD_LEN
///
/// ```
/// # use uds_services::{
/// #     Ai, DataIdentifier, DataTransfer, ReadDataByIdentifier, RecordError, Reloads,
/// #     ResponseSink, ServerParams, ServiceSet, Storage, Timestamp, TransferRequest,
/// #     TransportEvent, UdsTransport, uds_server,
/// # };
/// # use uds_protocol::NegativeResponseCode as Nrc;
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
/// #     const MAX_BLOCK_LENGTH: usize = 1_024;
/// #     const SUPPORTS_UPLOAD: bool = false;
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
/// #     async fn t_data_req(&mut self, _ai: Ai, _data: &[u8]) -> Result<(), ()> { Ok(()) }
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
/// # const PARAMS: ServerParams = ServerParams {
/// #     s3_server: 5_000,
/// #     p2_server_max: 50,
/// #     p2_star_server_max: 5_000,
/// # };
/// uds_server! {
///     Ecu: ReadDataByIdentifier, DataTransfer;
///     transport = DoIpTransport,
///     peers = 4,
///     server = EcuServer,
/// }
///
/// // Constructed in place: no stack temporary holds the buffers on the way in.
/// static SERVER: EcuServer = EcuServer::new(Ecu::new(), DoIpTransport, PARAMS);
///
/// let mut store = <<Ecu as ServiceSet>::Store as Storage>::EMPTY;
/// let buffers = store.split();
/// assert_eq!(buffers.in_flight.len(), 1_026);
/// assert_eq!(buffers.response.len(), 77);
/// # let _ = &SERVER;
/// ```
#[macro_export]
macro_rules! uds_server {
    (
        $ty:ty : $($svc:ident),+ $(,)? ;
        transport = $transport:ty,
        peers = $peers:expr,
        server = $server:ident $(,)?
    ) => {
        /// The assembled server: this application's services, storage, session layer and
        /// transport. Emitted by `uds_server!`.
        type $server = $crate::Server<$ty, $transport, { $peers }>;

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
            // Clause 8.7.6 admits only a two-byte TesterPresent and a 0x00-0x0F request
            // while a service is in progress. Anything larger is occupancy and is
            // answered busyRepeatRequest from its service identifier alone.
            //
            // No service `uds_server!` can currently assemble falls in 0x00-0x0F --
            // that range is OBD territory, which uds_protocol does not model -- so the
            // second exception is unreachable today. The arm in
            // `is_concurrent_exception` below is where it will be handled when a
            // service in that range arrives.
            const CONCURRENT: usize = 8;

            impl $crate::sealed::Sealed for $ty {}

            impl $crate::ServiceSet for $ty {
                type Store = $crate::Store<IN_FLIGHT, CONCURRENT, RESPONSE>;

                async fn dispatch(
                    &mut self,
                    request: &[u8],
                    out: &mut $crate::ResponseSink<'_>,
                ) -> ::core::result::Result<
                    $crate::Responded,
                    ::uds_protocol::NegativeResponseCode,
                > {
                    #[allow(
                        clippy::todo,
                        reason = "API stub; behaviour lands with its element"
                    )]
                    {
                        todo!(
                            "UDSSVC_ARCH_0004 pipeline: {} bytes, {} written",
                            request.len(),
                            out.written()
                        )
                    }
                }

                fn supports(&self, sid: u8) -> bool {
                    $( if $crate::__uds_sids!($svc).contains(&sid) { return true; } )+
                    false
                }

                fn may_respond_pending(&self, sid: u8) -> bool {
                    $( if $crate::__uds_sids!($svc).contains(&sid) {
                        return $crate::__uds_may_pend!($ty, $svc);
                    } )+
                    false
                }

                fn is_concurrent_exception(
                    &self,
                    request: &[u8],
                    ai: $crate::Ai,
                ) -> bool {
                    match request.first() {
                        // Clause 8.7.6's first exception is a functionally addressed
                        // TesterPresent whose sub-function carries
                        // suppressPosRspMsgIndication, which is bit 7 of that byte.
                        // `get` rather than an index: this crate denies
                        // indexing_slicing.
                        Some(0x3E) => {
                            ::core::matches!(ai.ta_type, $crate::TaType::Functional)
                                && request.get(1).is_some_and(|sub| sub & 0x80 != 0)
                        }
                        // Unreachable as the crate stands: no service `uds_server!`
                        // can assemble falls in 0x00-0x0F. Kept as the place the case
                        // will be handled when one does.
                        Some(sid) if *sid <= 0x0F => self.supports(*sid),
                        _ => false,
                    }
                }
            }
        };
    };
}

/// Assemble a client from the identifier vocabulary it speaks.
///
/// ``UDSSVC_ARCH_0020``, ``UDSSVC_ARCH_0024`` — the same enumeration a server's handlers
/// are written against names a client's requests, so a vehicle programme that builds both
/// from one definition gets a compile error when only one was updated.
///
/// The fold is [`crate::uds_server`]'s `ReadDataByIdentifier` arms, run against the
/// client's side of the same exchange: a request is the service identifier and two bytes
/// per identifier, and a response is the service identifier and each identifier beside
/// its record. Naming the transport lets its
/// [`MAX_PDU`](crate::UdsTransport::MAX_PDU) cap both, exactly as it does for a server.
///
/// `client = Name` is the alias the client is reached through, so the channel counts and
/// the keep-alive mode are written once, here, rather than at every signature that names
/// a client.
///
/// # Examples
///
/// ```
/// # use uds_services::{
/// #     Ai, DataIdentifier, PhysicalKeepAlive, RecordError, Reloads, Timestamp,
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
/// #     async fn t_data_req(&mut self, _ai: Ai, _d: &[u8]) -> Result<(), ()> { Ok(()) }
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
/// uds_client! {
///     Did;
///     transport = DoIpTransport,
///     max_dids_per_request = 4,
///     physical = 2,
///     functional = 1,
///     responders = 4,
///     keep_alive = PhysicalKeepAlive,
///     client = Tester,
/// }
///
/// static TESTER: Tester = Tester::new(DoIpTransport, PhysicalKeepAlive);
/// # let _ = &TESTER;
/// ```
#[macro_export]
macro_rules! uds_client {
    (
        $did:ty ;
        transport = $transport:ty,
        max_dids_per_request = $max_dids:expr,
        physical = $phys:expr,
        functional = $func:expr,
        responders = $responders:expr,
        keep_alive = $keep_alive:ty,
        client = $client:ident $(,)?
    ) => {
        /// The assembled client: this application's identifiers, its buffers, session
        /// layer and transport. Emitted by `uds_client!`.
        type $client = $crate::Client<
            $did,
            $transport,
            $keep_alive,
            { $phys },
            { $func },
            { $responders },
        >;

        const _: () = {
            const REQUEST: usize = $crate::assembly::min2(
                1 + 2 * { $max_dids },
                <$transport as $crate::UdsTransport>::MAX_PDU,
            );
            const RESPONSE: usize = $crate::assembly::min2(
                1 + { $max_dids } * (2 + <$did as $crate::DataIdentifier>::MAX_RECORD_LEN),
                <$transport as $crate::UdsTransport>::MAX_PDU,
            );

            impl $crate::sealed::Sealed for $did {}

            impl $crate::ClientSet for $did {
                type Store = $crate::ClientStore<REQUEST, RESPONSE>;
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::{dtc_response_bound, max_of, min2};
    use crate::DtcReportKind;

    /// The widest layout wins, and it is not always the one with the widest records: a
    /// `SeverityList` of ten beats a `DtcList` of ten on record width, and `Count` beats
    /// both when the server reports no records at all.
    #[test]
    fn the_widest_declared_layout_sets_the_bound() {
        const LIST: usize = dtc_response_bound(&[DtcReportKind::DtcList], 10);
        const SEVERITY: usize = dtc_response_bound(&[DtcReportKind::SeverityList], 10);
        const BOTH: usize =
            dtc_response_bound(&[DtcReportKind::DtcList, DtcReportKind::SeverityList], 10);
        assert_eq!((LIST, SEVERITY, BOTH), (3 + 40, 3 + 60, 3 + 60));
    }

    /// The header is part of the bound, so a layout carrying no records still needs
    /// room, and `WwhObdSeverity`'s five header bytes plus the `0x59` are counted.
    #[test]
    fn a_layout_without_records_still_needs_its_header() {
        const COUNT: usize = dtc_response_bound(&[DtcReportKind::Count], 10);
        const WWH: usize = dtc_response_bound(&[DtcReportKind::WwhObdSeverity], 10);
        assert_eq!((COUNT, WWH), (6, 6 + 50));
    }

    /// A server declaring no report types folds to zero, as an empty service set does.
    #[test]
    fn no_declared_layout_folds_to_zero() {
        const NONE: usize = dtc_response_bound(&[], 10);
        assert_eq!(NONE, 0);
    }

    /// The fold the macro performs, as a `const` so it fails to *compile* rather than
    /// to run if it ever stops being const-evaluable — which is the property that
    /// matters.
    #[test]
    fn the_maximum_is_const_evaluable() {
        const SIZES: [usize; 3] = [7, 1_026, 300];
        const MAX: usize = max_of(&SIZES);
        assert_eq!(MAX, 1_026);
    }

    /// An empty set folds to zero rather than panicking. A server with no services is
    /// useless but not malformed, and a panic in a const is a compile error with a very
    /// poor message.
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
