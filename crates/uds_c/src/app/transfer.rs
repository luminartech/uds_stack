//! `RequestDownload` (0x34), `TransferData` (0x36) and `RequestTransferExit` (0x37) over
//! the first layer that serves downloads.

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, DataFormatIdentifier, DataTransfer, ResponseSink, Sessions, TransferRequest,
};

use super::{Policy, TableApp, settled};
use crate::layer::Handler;
use crate::op;

impl<P: Policy, const RECORD: usize, const LEVELS: usize> DataTransfer
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_BLOCK_LEN: usize = P::MAX_BLOCK_LEN;
    const SESSIONS: Sessions = P::TRANSFER_SESSIONS;

    /// Where a layer serves downloads.
    fn serves(&self) -> bool {
        P::LAYERS.transfer().is_some()
    }

    fn format_supported(&self, _format: DataFormatIdentifier) -> bool {
        true
    }

    /// Downloads only, to 32-bit addresses.
    fn access(&self, request: &TransferRequest<'_>) -> Option<Access> {
        match *request {
            TransferRequest::Download { address, size, .. }
                if u32::try_from(address).is_ok() && u32::try_from(size).is_ok() =>
            {
                P::LAYERS.transfer().map(|t| t.access.access())
            }
            _ => None,
        }
    }

    async fn begin(&mut self, request: TransferRequest<'_>) -> Result<(), Nrc> {
        let (
            Some(begin),
            TransferRequest::Download {
                data_format,
                address,
                size,
            },
        ) = (
            P::LAYERS.transfer().and_then(|t| t.begin).map(Handler),
            request,
        )
        else {
            return Err(Nrc::RequestOutOfRange);
        };
        let (Ok(address), Ok(size)) = (u32::try_from(address), u32::try_from(size)) else {
            return Err(Nrc::RequestOutOfRange);
        };
        let format = u8::from(data_format);
        settled(op::settle(|op| begin.call(op, address, size, format)).await)
    }

    async fn block(&mut self, data: &[u8], _out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        let Some(block) = P::LAYERS.transfer().and_then(|t| t.block).map(Handler) else {
            return Err(Nrc::RequestSequenceError);
        };
        settled(op::settle(|op| block.call(op, data)).await)
    }

    async fn exit(
        &mut self,
        _record: &[u8],
        _out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let Some(finish) = P::LAYERS.transfer().and_then(|t| t.finish).map(Handler) else {
            return Err(Nrc::RequestSequenceError);
        };
        settled(op::settle(|op| finish.call(op)).await)
    }
}
