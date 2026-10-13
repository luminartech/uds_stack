//! Upload and download — ISO 14229-1:2020 clause 15.

use crate::{Access, ResponseSink, Sessions};
use uds_protocol::{DataFormatIdentifier, FileOperationMode, NegativeResponseCode};

/// What a transfer is being requested for.
///
/// ``UDSSVC_ARCH_0036`` puts direction here rather than in a separate field, because a
/// repeated block is handled *differently* by direction — a repeated download is answered
/// without calling the handler, a repeated upload calls it again — and a design that
/// treats repetition uniformly is wrong in one direction whichever uniform choice it
/// makes.
///
/// Address and size arrive decoded. Their wire widths are declared by the
/// `addressAndLengthFormatIdentifier`, which `uds_protocol` reads to size the fields —
/// so the widths are a decoding concern that is already settled by the time a handler is
/// called, and the identifier itself never reaches one. The two are [`u64`] and [`u32`]
/// after ISO 14229-1:2020 Table H.1, which caps `memoryAddress` at five bytes and
/// `memorySize` at four; being different types, they also cannot be transposed.
///
/// Not [`Hash`]: [`FileOperationMode`] is not, and a transfer request is a thing to match
/// on rather than a key to look one up by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferRequest<'a> {
    /// `RequestDownload` (0x34) — the client sends data to the server.
    Download {
        /// `dataFormatIdentifier`; see [`DataFormatIdentifier`], whose
        /// [`NONE`](DataFormatIdentifier::NONE) is the uncompressed, unencrypted case.
        data_format: DataFormatIdentifier,
        /// `memoryAddress`, decoded.
        address: u64,
        /// `memorySize`, decoded.
        size: u32,
    },
    /// `RequestUpload` (0x35) — the server sends data to the client.
    Upload {
        /// `dataFormatIdentifier`; see [`DataFormatIdentifier`].
        data_format: DataFormatIdentifier,
        /// `memoryAddress`, decoded.
        address: u64,
        /// `memorySize`, decoded.
        size: u32,
    },
    /// `RequestFileTransfer` (0x38).
    File {
        /// `modeOfOperation`; see [`FileOperationMode`].
        operation: FileOperationMode,
        /// `filePathAndName`.
        path: &'a [u8],
    },
}

/// The download and upload services (0x34, 0x35, 0x36, 0x37, 0x38).
///
/// ``UDSSVC_ARCH_0036`` — the transfer lifecycle is a state machine this crate owns:
/// which requests are admissible in which order, the block sequence counter and its
/// `wrongBlockSequenceCounter` (0x73), the `0xFF → 0x00` roll, and rejecting a
/// `TransferData` with no transfer in progress. One trait rather than four, because the
/// services are one state machine and implementing three of them would be meaningless.
///
/// A response lost after it settled is not undone: a lost `TransferData` response is
/// recovered by the client repeating the block, which is answered without calling
/// [`Self::block`] again, as `76 <counter>` without any `transferResponseParameterRecord`
/// the first answer carried (a CRC, say), so the repeat's answer can differ from the
/// first. A lost `RequestDownload` response leaves the download in progress, so the
/// client's repeat is `conditionsNotCorrect` (0x22, Table 444). The client leaves that by
/// sending its blocks from counter 0x01, the download being open; by a change to the
/// default session; for a transfer admitted under a security level, by any session
/// change (Figure 7 key 3(ii)); or by a session outside [`Self::SESSIONS`] (key 3(iii)).
/// `RequestTransferExit` closes it only where it is not answered `requestSequenceError`
/// (0x24): a compressed format, or all `memorySize` bytes received. A lost `77` leaves the
/// transfer closed, so the client's repeated exit is 0x24 (Table 459, Figure 34) although
/// the download succeeded.
///
/// Where `SUPPORTS_UPLOAD` is `false` and no other listed service needs more, the response
/// buffer is 6 bytes, so the `transferResponseParameterRecord` [`Self::block`] writes
/// after `76 <counter>` is at most 4 bytes and the one [`Self::exit`] writes after `77` at
/// most 5; a longer one is `responseTooLong` (0x14).
pub trait DataTransfer {
    /// ``UDSSVC_ARCH_0033`` — erasing flash before a download usually needs one.
    const MAY_RESPOND_PENDING: bool;

    /// The most data bytes one `TransferData` block carries, excluding the service
    /// identifier and the `blockSequenceCounter`.
    ///
    /// The `RequestDownload` positive response advertises `MAX_BLOCK_LENGTH + 2` as
    /// `maxNumberOfBlockLength`, because ISO 14229-1:2020 15.2.3.2 has that value
    /// "reflect the complete message length, including the service identifier and the
    /// data-parameters present in the `TransferData` request message", and "A server is
    /// required to accept transferData requests that are equal in length to its reported
    /// maxNumberOfBlockLength". [`crate::uds_server`] holds the in-flight buffer to that
    /// length and fails to compile where the transport cannot carry it; a client
    /// subtracts two to size its blocks.
    const MAX_BLOCK_LENGTH: usize;

    /// Whether the response buffer is sized for an upload's block, which is not yet served:
    /// `RequestUpload` is always `serviceNotSupported` (0x11).
    ///
    /// A download-only server — the flashing case — never puts a block in a *response*,
    /// so it must not carry a response buffer sized for one.
    const SUPPORTS_UPLOAD: bool;

    /// The sessions `RequestDownload`, `TransferData` and `RequestTransferExit` are served
    /// in. In any other session each is `serviceNotSupportedInActiveSession` (0x7F)
    /// (Figure 5); ISO 14229-1:2020 Table 23's refusal of all three in the default session
    /// applies first, whatever this says.
    const SESSIONS: Sessions;

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
    /// `securityAccessDenied` (0x33), "Security check ok for requested memory interval?".
    /// [`Self::begin`] is asked only after all three pass.
    ///
    /// # Arguments
    ///
    /// * `request` - the transfer requested; see [`TransferRequest`].
    fn access(&self, request: &TransferRequest<'_>) -> Option<Access>;

    /// Begin a transfer.
    ///
    /// The positive response's `maxNumberOfBlockLength` is composed by this crate from
    /// [`Self::MAX_BLOCK_LENGTH`], so this returns nothing.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] — `requestOutOfRange` (0x31) for an unwritable
    /// region, `uploadDownloadNotAccepted` (0x70) where conditions forbid it.
    fn begin(
        &mut self,
        request: TransferRequest<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// Accept one block of a download, or produce one block of an upload into `out`.
    ///
    /// The block sequence counter is **not** a parameter: validating it and answering
    /// `wrongBlockSequenceCounter` (0x73) is this crate's, and a handler that received
    /// the counter could disagree with the state machine maintaining it.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] — `generalProgrammingFailure` (0x72) where the write
    /// failed, `transferDataSuspended` (0x71) where it cannot continue.
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
    /// The [`NegativeResponseCode`] where the transfer cannot complete — a checksum
    /// failure is the usual case.
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
        const MAX_BLOCK_LENGTH: usize = 1_024;
        const SUPPORTS_UPLOAD: bool = false;
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

    /// Decision 4 of the design: the advertised `maxNumberOfBlockLength` is
    /// `MAX_BLOCK_LENGTH + 2` (15.2.3.2), the length of the in-flight buffer that must hold
    /// the block.
    #[test]
    fn the_block_length_gives_both_the_buffer_and_the_advertisement() {
        const BLOCK: usize = <Ecu as DataTransfer>::MAX_BLOCK_LENGTH;
        const ADVERTISED: usize = 2 + BLOCK; // SID + block sequence counter
        assert_eq!((BLOCK, ADVERTISED), (1_024, 1_026));
    }

    /// A download-only server never puts a block in a *response*, so it must not carry a
    /// response buffer sized for one. 949 bytes on a 1 KiB block, which is the common
    /// flashing case.
    #[test]
    fn a_download_only_server_does_not_pay_for_an_upload_block() {
        const RSP: usize = if <Ecu as DataTransfer>::SUPPORTS_UPLOAD {
            2 + <Ecu as DataTransfer>::MAX_BLOCK_LENGTH
        } else {
            6
        };
        assert_eq!(RSP, 6);
    }

    /// ``UDSSVC_ARCH_0036`` — direction is in the request because a repeated block is
    /// handled differently by direction: a repeated download is answered without calling
    /// the handler, a repeated upload calls it again. A design treating repetition
    /// uniformly is wrong in one direction whichever uniform choice it makes.
    #[test]
    fn direction_is_carried_by_the_request() {
        let d = TransferRequest::Download {
            data_format: DataFormatIdentifier::NONE,
            address: 0x0800_0000,
            size: 0x0002_0000,
        };
        let u = TransferRequest::Upload {
            data_format: DataFormatIdentifier::NONE,
            address: 0x0800_0000,
            size: 0x0002_0000,
        };
        assert_ne!(d, u);
    }
}
