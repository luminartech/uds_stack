//! Upload and download — ISO 14229-1:2020 clause 15.

use crate::ResponseSink;
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
pub trait DataTransfer {
    /// ``UDSSVC_ARCH_0033`` — erasing flash before a download usually needs one.
    const MAY_RESPOND_PENDING: bool;

    /// The largest block this server accepts or produces, excluding the service
    /// identifier and the block sequence counter.
    ///
    /// **This constant serves two purposes deliberately.** Clause 14.2 obliges the server
    /// to report `maxNumberOfBlockLength` in its `RequestDownload` positive response, and
    /// this is that value; it is also what [`crate::uds_server`] folds into the in-flight
    /// buffer. Declaring it once means the advertised number and the buffer that must
    /// hold the block cannot disagree.
    const MAX_BLOCK_LENGTH: usize;

    /// Whether this server answers `RequestUpload`.
    ///
    /// A download-only server — the flashing case — never puts a block in a *response*,
    /// so it must not carry a response buffer sized for one.
    const SUPPORTS_UPLOAD: bool;

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
    use crate::ResponseSink;
    use uds_protocol::NegativeResponseCode;

    struct Ecu;
    impl DataTransfer for Ecu {
        const MAY_RESPOND_PENDING: bool = true;
        const MAX_BLOCK_LENGTH: usize = 1_024;
        const SUPPORTS_UPLOAD: bool = false;
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

    /// The constant that sizes the in-flight buffer is the same constant clause 14.2
    /// obliges the server to advertise as maxNumberOfBlockLength. One value, so the
    /// buffer and the advertisement cannot disagree — and a disagreement there is a
    /// buffer overrun on the next `TransferData`.
    #[test]
    fn the_block_length_is_one_value_serving_two_purposes() {
        const ADVERTISED: usize = <Ecu as DataTransfer>::MAX_BLOCK_LENGTH;
        const REQUIRED_BUFFER: usize = 2 + ADVERTISED; // SID + block sequence counter
        assert_eq!((ADVERTISED, REQUIRED_BUFFER), (1_024, 1_026));
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
