//! `ReadDataByIdentifier` (0x22) and `WriteDataByIdentifier` (0x2E) over the layers' DID
//! tables, and the DID the server answers itself.

use core::fmt;
use core::marker::PhantomData;

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, DataIdentifier, ReadDataByIdentifier, RecordError, ResponseSink, Sink,
    WriteDataByIdentifier,
};

use super::{Policy, TableApp, settled};
use crate::layer::{Handler, sessions};
use crate::op;

/// `activeDiagnosticSessionDataIdentifier` (ISO 14229-1:2020 Table C.1), answered from
/// the server's own session state in every session; a table entry for it is never read.
pub const ACTIVE_SESSION_DID: u16 = 0xF186;

/// A data identifier the server or one of `P`'s layers serves.
pub struct Did<P>(u16, PhantomData<fn() -> P>);

impl<P> Did<P> {
    /// The identifier's value.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<P> Clone for Did<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Did<P> {}

impl<P> PartialEq for Did<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P> Eq for Did<P> {}

impl<P> fmt::Debug for Did<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Did({:#06X})", self.0)
    }
}

impl<P: Policy> DataIdentifier for Did<P> {
    /// A longer record is refused by [`TableApp::validate`].
    const MAX_RECORD_LEN: usize = P::MAX_DID_LEN;

    fn as_u16(self) -> u16 {
        self.0
    }

    fn from_u16(value: u16) -> Option<Self> {
        (value == ACTIVE_SESSION_DID || P::LAYERS.did(value).is_some())
            .then_some(Self(value, PhantomData))
    }

    /// A write's record is the entry's `len` bytes.
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        let len = P::LAYERS
            .did(self.0)
            .map_or(0, |entry| usize::from(entry.len));
        buf.split_at_checked(len).ok_or(RecordError::Short)
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ReadDataByIdentifier
    for TableApp<P, RECORD, LEVELS>
{
    type Did = Did<P>;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = P::MAX_DIDS_PER_REQUEST;

    /// Answers the entry's `len` bytes, zero where the handler copied nothing.
    async fn read(&mut self, did: Did<P>, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        if did.0 == ACTIVE_SESSION_DID {
            // A refused write is answered responseTooLong by the server.
            let _ = out.write_all(&[u8::from(self.session)]);
            return Ok(());
        }
        let Some((read, read_sessions, len)) = P::LAYERS.did(did.0).and_then(|e| {
            e.read
                .map(|read| (Handler(read), e.read_sessions, usize::from(e.len)))
        }) else {
            return Err(Nrc::RequestOutOfRange);
        };
        if !sessions(read_sessions).contains(self.session) {
            return Err(Nrc::RequestOutOfRange);
        }
        let mut record = self.answer(len, P::MAX_DID_LEN)?;
        settled(op::settle(|op| record.lend(|| read.call(op))).await)?;
        let _ = out.write_all(record.all());
        Ok(())
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> WriteDataByIdentifier
    for TableApp<P, RECORD, LEVELS>
{
    type Did = Did<P>;
    const MAY_RESPOND_PENDING: bool = true;

    fn access(&self, did: Did<P>) -> Option<Access> {
        P::LAYERS
            .did(did.0)
            .filter(|e| e.write.is_some())
            .map(|e| e.write_access.access())
    }

    async fn write(&mut self, did: Did<P>, record: &[u8]) -> Result<(), Nrc> {
        let Some(write) = P::LAYERS.did(did.0).and_then(|e| e.write).map(Handler) else {
            return Err(Nrc::RequestOutOfRange);
        };
        settled(op::settle(|op| write.call(op, record)).await)
    }
}
