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
        3 + <$ty as $crate::ReadDtcInformation>::MAX_DTCS
            * <$ty as $crate::ReadDtcInformation>::DTC_RECORD_LEN
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
    ($ty:ty, $svc:ident) => {
        <$ty as $crate::$svc>::MAY_RESPOND_PENDING
    };
}

/// Assemble a server from the services it implements.
///
/// ``UDSSVC_ARCH_0013``, ``UDSSVC_ARCH_0035``.
///
/// ```ignore
/// uds_server! {
///     Ecu: ReadDataByIdentifier, SecurityAccess, DataTransfer;
///     transport = DoIpTransport<TcpSocket>,
///     peers = 4,
///     server = EcuServer,
/// }
///
/// static SERVER: EcuServer = EcuServer::new(Ecu::new(), transport, PARAMS);
/// ```
///
/// Naming the transport lets its [`MAX_PDU`](crate::UdsTransport::MAX_PDU) join the fold.
/// It introduces no dependency on a binding: the *application* names the type, and it
/// already depends on both crates, so ``UDSSVC_ARCH_0002`` and ``0003`` are untouched.
///
/// `peers = N` sizes the association array of the `uds_session::Server<N>` the driver
/// owns, and `server = Name` is the alias it is reached through — the macro emits
/// `type Name = Server<Ecu, Transport, N>`, so the count is written once, where it acts.
/// It was `channels = N` and sized nothing at all: an application wrote the number here,
/// where the expansion discarded it, and again as [`crate::Server`]'s third parameter,
/// where it did the work, with no diagnostic when the two disagreed. The rename also
/// clears a collision — a *channel* in `uds_session` is a client's physical or functional
/// channel, which is a different thing from a server's peer.
///
/// The syntax is `Ecu: ..; transport = T, ..` rather than `Ecu over T: ..` because
/// `$ty:ty` cannot be followed by a bare identifier — the legal followers are
/// `{ [ => , > = : ; | as where`.
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

#[cfg(test)]
mod tests {
    use super::{max_of, min2};

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
