//! The client role.
//!
//! ``UDSSVC_ARCH_0020`` — a client issues requests over the application's own
//! identifiers, and ``UDSSVC_ARCH_0024`` makes that the same vocabulary a server's
//! handlers are written against. A client implements no service trait.
//!
//! ``UDSSVC_ARCH_0021`` — a negative response is interpreted here. The layer below
//! declines it in writing, and there is no layer between the two.
//!
//! **The storage shape mirrors `uds_session`'s.** That crate splits channels by kind:
//! `PhysicalSlot` carries no responder table where `FunctionalSlot<R>` does, and a
//! physical channel's `tS3_Client` is an argument of `open_physical_channel` in physical
//! keep-alive and absent from it in functional keep-alive.
//! Mirroring the split costs a third const parameter and buys the same thing it buys
//! them — a physical-only client pays nothing for responder tables, and a functional
//! channel cannot be given a session reload it has no use for.
//!
//! **Unverified.** `uds_on_ip`'s client is entirely `todo!()`, so this half cannot be
//! exercised end to end yet (open question 7). Its shape is settled enough to design
//! against; its behaviour is not.

use crate::{DataIdentifier, RecordError, UdsTransport};
use uds_protocol::NegativeResponseCode;
use uds_session::{Address, KeepAlive};

/// What one server said.
///
/// ``UDSSVC_ARCH_0021`` and ``UDSSVC_ARCH_0023`` — three cases, and none is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response<V> {
    /// A positive response, decoded.
    Positive(V),
    /// A negative response, decoded. Not an error: a server answering
    /// `serviceNotSupported` has answered.
    Negative(NegativeResponseCode),
    /// The exchange completed and nothing came back — the suppress bit, or a
    /// functionally addressed request no server supports. Distinct from a timeout,
    /// because a timeout is a fault and suppression is not.
    NoResponseExpected,
}

/// One server's answer, with the server that gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer<V> {
    /// The responding server's `S_AI[SA]`.
    ///
    /// ``UDSSVC_ARCH_0022`` — for a functionally addressed request each responding server
    /// sets its own source address, and that is how responses are told apart.
    pub from: Address,
    /// What it said.
    pub response: Response<V>,
}

/// The answers to one functionally addressed request.
///
/// ``UDSSVC_ARCH_0022`` — a functional request reaches every server on the bus, so zero
/// or more may answer, and by ``UDSSVC_ARCH_0009`` those that do not support it answer
/// with silence. There is no single response to return, so typing one would force the API
/// to lie about the common case.
///
/// The sequence is **lending**: an answer borrows the receive buffer and is valid only
/// until the next is taken. ``UDSSVC_ARCH_0017``'s reasoning — an owned sequence allocates
/// per response. An inherent `async fn` rather than a `Stream`, for the same reason
/// `uds_on_ip`'s equivalent is not one: a `Stream` item cannot borrow the receive buffer.
#[derive(Debug)]
#[must_use = "an undrained sequence discards the answers the request produced"]
pub struct Responses<
    'c,
    T: UdsTransport,
    K: KeepAlive,
    D: DataIdentifier,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> {
    client: &'c mut Client<T, K, PHYS, FUNC, R>,
    _marker: core::marker::PhantomData<D>,
}

impl<
    T: UdsTransport,
    K: KeepAlive,
    D: DataIdentifier,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> Responses<'_, T, K, D, PHYS, FUNC, R>
{
    /// The next answer, or `None` when the response window has closed.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed.
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async is load-bearing: the body awaits the transport once it \
                  replaces this todo!(), and neither lint can see past the stub"
    )]
    pub async fn next(&mut self) -> Option<Result<Answer<Records<'_, D>>, T::Error>> {
        #[allow(clippy::todo, reason = "API stub; behaviour lands with its element")]
        {
            let _ = &mut *self.client;
            todo!("UDSSVC_ARCH_0022: lend the next decoded answer")
        }
    }
}

/// The identifier/record pairs in one `ReadDataByIdentifier` response.
///
/// ``UDSSVC_ARCH_0026`` — the application declared each identifier's record structure, so
/// splitting a multi-identifier response needs nothing this crate knows about the data.
/// A response is a concatenation of `(identifier, record)` with no length prefixes, and
/// [`DataIdentifier::split_record`] is what makes it walkable.
///
/// Items borrow the receive buffer, not the iterator, so this is an ordinary [`Iterator`]
/// rather than a lending one. `Clone` but not `Copy`: a copied iterator silently restarts
/// the walk, which is the whole hazard `clippy::copy_iterator` names.
#[derive(Debug, Clone)]
#[must_use = "the records are the response; dropping this discards it"]
pub struct Records<'d, D> {
    rest: &'d [u8],
    identifier: core::marker::PhantomData<fn() -> D>,
}

impl<'d, D: DataIdentifier> Records<'d, D> {
    /// Walk `response`, the bytes after the `0x62` service identifier.
    pub const fn new(response: &'d [u8]) -> Self {
        Self {
            rest: response,
            identifier: core::marker::PhantomData,
        }
    }
}

impl<'d, D: DataIdentifier> Iterator for Records<'d, D> {
    type Item = Result<(D, &'d [u8]), RecordError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let Some((identifier, tail)) = self.rest.split_first_chunk::<2>() else {
            self.rest = &[];
            return Some(Err(RecordError::Short));
        };
        let Some(did) = D::from_u16(u16::from_be_bytes(*identifier)) else {
            self.rest = &[];
            return Some(Err(RecordError::UnknownIdentifier));
        };
        match did.split_record(tail) {
            Ok((record, rest)) => {
                self.rest = rest;
                Some(Ok((did, record)))
            }
            Err(e) => {
                self.rest = &[];
                Some(Err(e))
            }
        }
    }
}

/// A UDS client over any transport.
///
/// ``UDSSVC_ARCH_0028`` — the core is sans-io; awaiting is the layer above it, so the
/// encode and interpret halves are usable without a transport at all.
///
/// `K`, `PHYS`, `FUNC` and `R` mirror `uds_session::Client<K, PHYS, FUNC, R>`: the
/// keep-alive mode, physical channels, functional channels, and responders per functional
/// channel. `R = 0` gives a physical-only client zero-sized responder tables.
///
/// `K` is `uds_session::FunctionalKeepAlive` or `PhysicalKeepAlive`, sealed there to those
/// two. Holding it in the type is that crate's `UDSS_LLR_0149`: the mode is fixed at
/// creation, so the methods that supply a `tS3_Client` reload exist only on the mode that
/// gives one a meaning.
#[derive(Debug)]
pub struct Client<
    T: UdsTransport,
    K: KeepAlive,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> {
    session: uds_session::Client<K, PHYS, FUNC, R>,
    transport: T,
}

impl<T: UdsTransport, K: KeepAlive, const PHYS: usize, const FUNC: usize, const R: usize>
    Client<T, K, PHYS, FUNC, R>
{
    /// A client over `transport`, owning its session-layer storage by value.
    ///
    /// The slot arrays are the caller's, supplied by value, so no lifetime reaches a
    /// consuming application — the same shape `Server::new` uses, and the reason
    /// `uds_session` moved its storage off references.
    pub const fn new(
        transport: T,
        physical: [uds_session::PhysicalSlot; PHYS],
        functional: [uds_session::FunctionalSlot<R>; FUNC],
        keep_alive: K,
    ) -> Self {
        Self {
            session: uds_session::Client::new(physical, functional, keep_alive),
            transport,
        }
    }

    /// Read one or more data identifiers from one server.
    ///
    /// The response comes back as [`Records`], the identifier/record pairs it carries,
    /// not as the undivided bytes: the application already declared each record's
    /// structure through [`DataIdentifier::split_record`], so re-walking the response by
    /// hand would be the caller redoing work this crate can do.
    ///
    /// # Errors
    ///
    /// [`UdsTransport::Error`] where the transport failed. A negative response is **not**
    /// an error — it arrives as [`Response::Negative`].
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "async is load-bearing: the body awaits the transport once it \
                  replaces this todo!(), and neither lint can see past the stub"
    )]
    pub async fn read_data_by_identifier<D: DataIdentifier>(
        &mut self,
        target: Address,
        identifiers: &[D],
    ) -> Result<Response<Records<'_, D>>, T::Error> {
        #[allow(clippy::todo, reason = "API stub; behaviour lands with its element")]
        {
            let _ = (
                &mut self.transport,
                &mut self.session,
                target,
                identifiers.len(),
            );
            todo!("UDSSVC_ARCH_0020: encode, exchange, interpret")
        }
    }

    /// Read one or more data identifiers from every server on a functional address.
    ///
    /// ``UDSSVC_ARCH_0022`` — a functional request reaches every server, so zero or
    /// more may answer and there is no single response to return. The answers come back
    /// as a lending sequence: this is the only way to obtain a [`Responses`], and
    /// draining it is how each server's answer is read. Failures and negative responses
    /// both surface there — a transport error per answer, and a negative response as
    /// [`Response::Negative`], which is not an error.
    pub fn read_data_by_identifier_functional<D: DataIdentifier>(
        &mut self,
        target: Address,
        identifiers: &[D],
    ) -> Responses<'_, T, K, D, PHYS, FUNC, R> {
        #[allow(clippy::todo, reason = "API stub; behaviour lands with its element")]
        {
            let _ = (&mut self.transport, target, identifiers.len());
            todo!("UDSSVC_ARCH_0022: encode and open the response window")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, Records, Response};
    use crate::{DataIdentifier, RecordError};
    use uds_protocol::NegativeResponseCode;
    use uds_session::Address;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestDid {
        VehicleSpeed,
        VinNumber,
    }

    impl DataIdentifier for TestDid {
        const MAX_RECORD_LEN: usize = 17;
        fn as_u16(self) -> u16 {
            match self {
                Self::VehicleSpeed => 0xF4_0D,
                Self::VinNumber => 0xF1_90,
            }
        }
        fn from_u16(value: u16) -> Option<Self> {
            match value {
                0xF4_0D => Some(Self::VehicleSpeed),
                0xF1_90 => Some(Self::VinNumber),
                _ => None,
            }
        }
        fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
            let w = match self {
                Self::VehicleSpeed => 1,
                Self::VinNumber => 17,
            };
            buf.split_at_checked(w).ok_or(RecordError::Short)
        }
    }

    /// ``UDSSVC_ARCH_0021`` — a negative response is a response, and this crate is the
    /// layer that interprets it. `uds_on_ip` says so from the other side: "A UDS
    /// negative response is not an error: it is a response, and interpreting it belongs
    /// to a higher layer." This is that layer.
    ///
    /// ``UDSSVC_ARCH_0023`` — and "completed, nothing came back" is a third case, not a
    /// timeout: the suppress bit, or a functional request no server supports. A timeout
    /// is a fault; suppression is not.
    #[test]
    fn a_response_has_three_cases_and_none_of_them_is_an_error() {
        let p: Response<&[u8]> = Response::Positive(&[0x62]);
        let n: Response<&[u8]> =
            Response::Negative(NegativeResponseCode::ServiceNotSupported);
        let q: Response<&[u8]> = Response::NoResponseExpected;
        assert!(matches!(p, Response::Positive(_)));
        assert!(matches!(n, Response::Negative(_)));
        assert!(matches!(q, Response::NoResponseExpected));
    }

    /// ``UDSSVC_ARCH_0022`` — each answer carries the source address of the server that
    /// produced it. For a functionally addressed request every responding server sets
    /// its own, and a sequence of decoded responses without their senders would be
    /// unattributable.
    #[test]
    fn every_answer_names_its_sender() {
        let a = Answer {
            from: Address(0x0E01),
            response: Response::Positive(&[0x62][..]),
        };
        assert_eq!(a.from, Address(0x0E01));
    }

    /// ``UDSSVC_ARCH_0026`` — the application declared the record widths, so the client
    /// hands back typed pairs rather than the undivided response. Two identifiers of
    /// different widths in one response is the case that makes the point: nothing but
    /// `split_record` can tell where the first record ends.
    #[test]
    fn a_response_splits_into_the_records_the_application_declared() {
        // 0xF40D and its one byte, then 0xF190 and its seventeen.
        let mut bytes = [0_u8; 22];
        let header = [0xF4, 0x0D, 0x40, 0xF1, 0x90];
        let Some(front) = bytes.get_mut(..header.len()) else {
            return;
        };
        front.copy_from_slice(&header);

        let mut records = Records::<TestDid>::new(&bytes);
        assert_eq!(
            records.next(),
            Some(Ok((TestDid::VehicleSpeed, &[0x40][..])))
        );
        assert_eq!(
            records.next(),
            Some(Ok((TestDid::VinNumber, &[0x00; 17][..])))
        );
        assert_eq!(records.next(), None);
    }

    /// An empty response yields nothing rather than an error: a server that answered
    /// positively with no records has answered.
    #[test]
    fn an_empty_response_yields_no_records() {
        assert_eq!(Records::<TestDid>::new(&[]).count(), 0);
    }

    /// A record shorter than the application declared is `Short`, and the walk stops —
    /// once the framing is lost there is no next identifier to find.
    #[test]
    fn a_truncated_record_ends_the_walk() {
        let mut records = Records::<TestDid>::new(&[0xF1, 0x90, 0x00, 0x00]);
        assert_eq!(records.next(), Some(Err(RecordError::Short)));
        assert_eq!(records.next(), None);
    }

    /// An identifier this application never defined is its own error. On a request that
    /// is `requestOutOfRange`; in a response it is a server naming something unasked for.
    #[test]
    fn an_identifier_the_application_does_not_define_is_reported() {
        let mut records = Records::<TestDid>::new(&[0xDE, 0xAD, 0x00]);
        assert_eq!(records.next(), Some(Err(RecordError::UnknownIdentifier)));
        assert_eq!(records.next(), None);
    }
}
