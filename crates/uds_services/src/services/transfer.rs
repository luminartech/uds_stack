//! Upload and download, ISO 14229-1:2020 clause 15.

use crate::{Access, ResponseSink, Sessions};
use uds_protocol::{
    DataFormatIdentifier, NegativeResponseCode, RequestFileTransferRequest,
};

/// What a transfer is being requested for.
///
/// ``UDSSVC_ARCH_0036`` puts direction here: a repeated download block is answered
/// without calling the handler, where a repeated upload block would call it again.
///
/// Address and size arrive decoded, each up to eight bytes wide and so a [`u64`]; the
/// `addressAndLengthFormatIdentifier` that declared their widths is `uds_protocol`'s to
/// read and never reaches a handler. A request declaring a wider one is answered
/// `requestOutOfRange` (0x31) before any handler is asked.
///
/// Not [`Hash`]: [`RequestFileTransferRequest`] is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransferRequest<'a> {
    /// `RequestDownload` (0x34): the client sends data to the server.
    Download {
        /// `dataFormatIdentifier`; see [`DataFormatIdentifier`], whose
        /// [`NONE`](DataFormatIdentifier::NONE) is the uncompressed, unencrypted case.
        data_format: DataFormatIdentifier,
        /// `memoryAddress`, decoded.
        address: u64,
        /// `memorySize`, decoded.
        size: u64,
    },
    /// `RequestFileTransfer` (0x38), as decoded: each `modeOfOperation` with the
    /// parameters ISO 14229-1:2020 Table 480 gives it. Not yet served, so never built.
    File(RequestFileTransferRequest<'a>),
}

/// The download services (0x34, 0x36, 0x37), one trait because they are one state
/// machine. `RequestUpload` (0x35) and `RequestFileTransfer` (0x38) are not served: both
/// are `serviceNotSupported` (0x11).
///
/// ``UDSSVC_ARCH_0036``: the transfer lifecycle is this crate's, which requests are
/// admissible in which order, the block sequence counter with its 0x73 and its
/// `0xFF -> 0x00` roll, and a `TransferData` with no transfer in progress. A response
/// lost after it settled is not undone: the client's repeat is answered from the state
/// the lost response left. Unless another listed service needs more, the response
/// buffer is 6 bytes, so the `transferResponseParameterRecord` [`Self::block`] writes
/// after `76 <counter>` is at most 4 bytes and the one [`Self::exit`] writes after `77`
/// at most 5; a longer one is `responseTooLong` (0x14).
pub trait DataTransfer {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The most data bytes one `TransferData` block carries, excluding the service
    /// identifier and the `blockSequenceCounter`.
    ///
    /// The `RequestDownload` positive response advertises `MAX_BLOCK_LEN + 2` as
    /// `maxNumberOfBlockLength`, the whole `TransferData` request message a server must
    /// accept (ISO 14229-1:2020 15.2.3.2). [`crate::uds_server`] holds the in-flight
    /// buffer to that length and fails to compile where the transport cannot carry it, or
    /// where this is zero; a client subtracts two to size its blocks.
    const MAX_BLOCK_LEN: usize;

    /// The sessions `RequestDownload`, `TransferData` and `RequestTransferExit` are served
    /// in. In any other session each is `serviceNotSupportedInActiveSession` (0x7F)
    /// (Figure 5); ISO 14229-1:2020 Table 23's refusal of all three in the default session
    /// applies first, whatever this says.
    ///
    /// This is the service's session set, asked before a request is decoded. A memory
    /// interval's [`Access::sessions`] is asked of a `RequestDownload` after it, and
    /// answers 0x31 (Figure 31). `TransferData` and `RequestTransferExit` name no
    /// interval, so this is their only session check. An open transfer is served in the
    /// sessions both allow, and ends in any other (Figure 7 key 3(iii)).
    const SESSIONS: Sessions;

    /// Whether this server serves downloads at all, for an application that learns it
    /// only at run time, from tables it is given, say. `true` by default.
    ///
    /// ISO 14229-1:2020 8.7.2 Figure 5, ``UDSSVC_ARCH_0006``: `false` settles every
    /// request for any of the three services `serviceNotSupported` (0x11), as for a
    /// service the assembly does not list, before any other check and without anything
    /// else here being asked.
    #[must_use]
    fn serves(&self) -> bool {
        true
    }

    /// Whether `format` is a `dataFormatIdentifier` this server accepts. Figure 31 asks it
    /// before the request's full length; `false` is `requestOutOfRange` (0x31).
    ///
    /// # Arguments
    ///
    /// * `format` - the request's [`DataFormatIdentifier`]; [`DataFormatIdentifier::NONE`]
    ///   is uncompressed and unencrypted.
    fn format_supported(&self, format: DataFormatIdentifier) -> bool;

    /// Where the memory interval `request` names may be transferred and which levels
    /// unlock it, or `None` where its `memoryAddress` or `memorySize` is not valid
    /// (``UDSSVC_ARCH_0044``).
    ///
    /// ISO 14229-1:2020 Figure 31, in its order: `None` is `requestOutOfRange` (0x31), an
    /// active session outside [`Access::sessions`] is 0x31 too, as for an identifier
    /// (Figures 26 and 30), and none of [`Access::levels`] unlocked is
    /// `securityAccessDenied` (0x33). [`Self::begin`] is asked only after all three pass.
    ///
    /// # Arguments
    ///
    /// * `request` - the transfer requested; see [`TransferRequest`].
    fn access(&self, request: &TransferRequest<'_>) -> Option<Access>;

    /// Begin a transfer.
    ///
    /// The positive response's `maxNumberOfBlockLength` is composed by this crate from
    /// [`Self::MAX_BLOCK_LEN`], so this returns nothing.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`]: `requestOutOfRange` (0x31) for an unwritable
    /// region, `uploadDownloadNotAccepted` (0x70) where conditions forbid it.
    fn begin(
        &mut self,
        request: TransferRequest<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// Accept one block of a download, writing any `transferResponseParameterRecord` into
    /// `out`. The block sequence counter is not a parameter: validating it and answering
    /// `wrongBlockSequenceCounter` (0x73) is this crate's.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`]: `generalProgrammingFailure` (0x72) where the write
    /// failed, `transferDataSuspended` (0x71) where it cannot continue, which ends the
    /// transfer.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn block(
        &mut self,
        data: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// End the transfer, writing any `transferResponseParameterRecord` into `out`.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where the transfer cannot complete, such as a
    /// checksum failure.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn exit(
        &mut self,
        parameter_record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the fixture's three handlers never await anything, since they exist only \
              to name a concrete type for the trait's associated constants — not a real \
              defect in test code"
)]
mod tests {
    use super::{DataFormatIdentifier, DataTransfer, TransferRequest};
    use crate::{Access, ResponseSink, Sessions};
    use uds_protocol::NegativeResponseCode;

    struct Ecu;
    impl DataTransfer for Ecu {
        const MAY_RESPOND_PENDING: bool = true;
        const MAX_BLOCK_LEN: usize = 1_024;
        const SESSIONS: Sessions = Sessions::ALL;
        fn format_supported(&self, _f: DataFormatIdentifier) -> bool {
            true
        }
        fn access(&self, _r: &TransferRequest<'_>) -> Option<Access> {
            Some(Access::new(Sessions::ALL))
        }
        async fn begin(
            &mut self,
            _r: TransferRequest<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
        async fn block(
            &mut self,
            _d: &[u8],
            _o: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
        async fn exit(
            &mut self,
            _r: &[u8],
            _o: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
    }

    /// The advertised `maxNumberOfBlockLength` is `MAX_BLOCK_LEN + 2` (15.2.3.2), the
    /// length of the in-flight buffer that holds the block.
    #[test]
    fn the_block_length_gives_both_the_buffer_and_the_advertisement() {
        const BLOCK: usize = <Ecu as DataTransfer>::MAX_BLOCK_LEN;
        const ADVERTISED: usize = 2 + BLOCK; // SID + block sequence counter
        assert_eq!((BLOCK, ADVERTISED), (1_024, 1_026));
    }
}
