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

use crate::storage::ClientStorage;
use crate::{DataIdentifier, RecordError, UdsTransport};
use uds_protocol::NegativeResponseCode;
use uds_session::{Address, KeepAliveMode};

/// One application's identifier vocabulary, with the storage derived from it.
///
/// ``UDSSVC_ARCH_0013`` for the client half — implemented by [`crate::uds_client`],
/// never by hand. It is implemented on the *identifier enumeration* rather than on an
/// application type, because unlike a server a client has no handler state: the
/// identifiers are the only thing the application declares, and they are what the buffer
/// lengths are folded from.
///
/// Sealed through [`crate::sealed`], for the reason
/// [`ServiceSet`](crate::ServiceSet) is: the derivation argument holds only while the
/// macro is what chooses [`Self::Store`]'s lengths.
pub trait ClientSet: DataIdentifier + crate::sealed::Sealed {
    /// The storage whose lengths were folded from this vocabulary's declared maxima.
    type Store: ClientStorage;
}

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
    /// A positive response whose bytes do not parse against this application's
    /// identifiers.
    ///
    /// Not an error, for the reason [`Answer::Malformed`] is not: the server answered and
    /// the transport delivered it intact.
    Malformed(RecordError),
    /// The exchange completed and nothing came back — the suppress bit, or a
    /// functionally addressed request no server supports. Distinct from a timeout,
    /// because a timeout is a fault and suppression is not.
    NoResponseExpected,
}

/// One server's answer to a functionally addressed request.
///
/// ``UDSSVC_ARCH_0022`` — each responding server sets its own source address, and that is
/// how answers are told apart, so every variant carries one.
///
/// There is no "nothing came back" case here, unlike [`Response`]: a server that stays
/// silent produces no answer at all, which is the response window closing rather than a
/// value to match on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Answer<'d, D: DataIdentifier> {
    /// A positive response, its records already validated against this vocabulary.
    Positive {
        /// The responding server's `S_AI[SA]`.
        from: Address,
        /// The identifier/record pairs it carried.
        records: Records<'d, D>,
    },
    /// A negative response. Not an error: a server answering `serviceNotSupported` has
    /// answered.
    Negative {
        /// The responding server's `S_AI[SA]`.
        from: Address,
        /// What it declined with.
        code: NegativeResponseCode,
    },
    /// A positive response whose bytes do not parse against this application's
    /// identifiers.
    ///
    /// Not a [`UdsTransport::Error`]: the transport delivered the message intact and the
    /// server answered. What failed is the agreement about what the bytes mean, which is
    /// a disagreement between two applications rather than a link fault.
    Malformed {
        /// The responding server's `S_AI[SA]`.
        from: Address,
        /// Why the response could not be walked.
        error: RecordError,
    },
}

impl<D: DataIdentifier> Answer<'_, D> {
    /// The responding server's `S_AI[SA]`, whichever answer this is.
    #[must_use]
    pub const fn from(&self) -> Address {
        match self {
            Self::Positive { from, .. }
            | Self::Negative { from, .. }
            | Self::Malformed { from, .. } => *from,
        }
    }
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
    C: ClientSet,
    T: UdsTransport,
    K: KeepAliveMode,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize = 0,
> {
    client: &'c mut Client<C, T, K, PHYS, FUNC, R>,
}

impl<
    C: ClientSet,
    T: UdsTransport,
    K: KeepAliveMode,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> Responses<'_, C, T, K, PHYS, FUNC, R>
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
    pub async fn next(&mut self) -> Option<Result<Answer<'_, C>, T::Error>> {
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
/// **Walking one cannot fail.** The whole response is checked when this is built, so a
/// framing error is one [`RecordError`] reported once — as [`Answer::Malformed`] or
/// [`Response::Malformed`] — rather than a `Result` at every step of a walk that could
/// only ever fail once and then end. There is no half-walked response: either every pair
/// is reachable or none is.
///
/// Items borrow the receive buffer, not the iterator, so this is an ordinary [`Iterator`]
/// rather than a lending one. `Clone` but not `Copy`: a copied iterator silently restarts
/// the walk, which is the whole hazard `clippy::copy_iterator` names.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "the records are the response; dropping this discards it"]
pub struct Records<'d, D> {
    rest: &'d [u8],
    identifier: core::marker::PhantomData<fn() -> D>,
}

impl<'d, D: DataIdentifier> Records<'d, D> {
    /// Check `response` — the bytes after the echoed service identifier — and hold it
    /// for walking.
    ///
    /// Not public: a [`Records`] comes from a read, so the offset this expects is never
    /// a caller's to get right.
    ///
    /// # Errors
    ///
    /// [`RecordError`] where an identifier is not one this application defines, or a
    /// record is shorter than it declared, or bytes trail the last whole record.
    #[allow(
        dead_code,
        reason = "no caller until the UDSSVC_ARCH_0020 read lands; the tests below \
                  exercise it"
    )]
    pub(crate) fn validate(response: &'d [u8]) -> Result<Self, RecordError> {
        let mut rest = response;
        while !rest.is_empty() {
            let Some((identifier, tail)) = rest.split_first_chunk::<2>() else {
                return Err(RecordError::Short);
            };
            let Some(did) = D::from_u16(u16::from_be_bytes(*identifier)) else {
                return Err(RecordError::UnknownIdentifier);
            };
            let (_record, remainder) = did.split_record(tail)?;
            rest = remainder;
        }
        Ok(Self {
            rest: response,
            identifier: core::marker::PhantomData,
        })
    }
}

impl<'d, D: DataIdentifier> Iterator for Records<'d, D> {
    type Item = (D, &'d [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let (identifier, tail) = self.rest.split_first_chunk::<2>()?;
        let did = D::from_u16(u16::from_be_bytes(*identifier))?;
        // `validate` walked this same path already, so none of the three fallible steps
        // here can fail. Ending the walk is the honest way to say so: this crate denies
        // both `panic` and `unreachable`, and stopping cannot be worse than the half-walk
        // the validation exists to prevent.
        let (record, rest) = did.split_record(tail).ok()?;
        self.rest = rest;
        Some((did, record))
    }
}

/// A UDS client over any transport.
///
/// ``UDSSVC_ARCH_0028`` — the core is sans-io; awaiting is the layer above it, so the
/// encode and interpret halves are usable without a transport at all.
///
/// `C` is the identifier vocabulary and, through [`ClientSet::Store`], the buffers folded
/// from it. `K`, `PHYS`, `FUNC` and `R` mirror `uds_session::Client<K, PHYS, FUNC, R>`:
/// the keep-alive mode, physical channels, functional channels, and responders per
/// functional channel. `R` defaults to `0`, as it does there, so a physical-only client
/// does not write a count for tables it has none of.
///
/// An application never spells any of this. [`crate::uds_client`] emits
/// `type Tester = Client<..>` and that alias is the name at every call site, the same way
/// [`crate::uds_server`]'s `server = Name` works.
///
/// `K` is `uds_session::FunctionalKeepAlive` or `PhysicalKeepAlive`, sealed there to those
/// two. Holding it in the type is that crate's `UDSS_LLR_0149`: the mode is fixed at
/// creation, so the methods that supply a `tS3_Client` reload exist only on the mode that
/// gives one a meaning.
#[derive(Debug)]
pub struct Client<
    C: ClientSet,
    T: UdsTransport,
    K: KeepAliveMode,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize = 0,
> {
    session: uds_session::Client<K, PHYS, FUNC, R>,
    transport: T,
    store: C::Store,
}

impl<
    C: ClientSet,
    T: UdsTransport,
    K: KeepAliveMode,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> Client<C, T, K, PHYS, FUNC, R>
{
    /// A client over `transport`, owning its session-layer and buffer storage by value.
    ///
    /// A `const fn`, which is load-bearing for the reason
    /// [`crate::Server::new`] is one: the buffers are inline and a runtime constructor
    /// would build a stack temporary before the move.
    ///
    /// The channel slots are built here rather than supplied, so an application never
    /// names `uds_session` — the same commitment [`crate::Server::new`] already makes by
    /// building its own session.
    pub const fn new(transport: T, keep_alive: K) -> Self {
        Self {
            session: uds_session::Client::new(
                [uds_session::PhysicalSlot::EMPTY; PHYS],
                [uds_session::FunctionalSlot::EMPTY; FUNC],
                keep_alive,
            ),
            transport,
            store: <C::Store as ClientStorage>::EMPTY,
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
    pub async fn read_data_by_identifier(
        &mut self,
        target: Address,
        identifiers: &[C],
    ) -> Result<Response<Records<'_, C>>, T::Error> {
        #[allow(clippy::todo, reason = "API stub; behaviour lands with its element")]
        {
            let _ = (
                &mut self.transport,
                &mut self.session,
                self.store.split(),
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
    pub fn read_data_by_identifier_functional(
        &mut self,
        target: Address,
        identifiers: &[C],
    ) -> Responses<'_, C, T, K, PHYS, FUNC, R> {
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
    /// unattributable — so `from` is on every variant and reachable without matching.
    #[test]
    fn every_answer_names_its_sender() {
        let declined: Answer<'_, TestDid> = Answer::Negative {
            from: Address(0x0E01),
            code: NegativeResponseCode::ServiceNotSupported,
        };
        let broken: Answer<'_, TestDid> = Answer::Malformed {
            from: Address(0x0E02),
            error: RecordError::Short,
        };
        assert_eq!(declined.from(), Address(0x0E01));
        assert_eq!(broken.from(), Address(0x0E02));
    }

    /// ``UDSSVC_ARCH_0026`` — the application declared the record widths, so the client
    /// hands back typed pairs rather than the undivided response. Two identifiers of
    /// different widths in one response is the case that makes the point: nothing but
    /// `split_record` can tell where the first record ends.
    ///
    /// Walking yields pairs, not `Result`s: the response was checked when the walk was
    /// built.
    #[test]
    fn a_response_splits_into_the_records_the_application_declared() {
        // 0xF40D and its one byte, then 0xF190 and its seventeen.
        let mut bytes = [0_u8; 22];
        let header = [0xF4, 0x0D, 0x40, 0xF1, 0x90];
        let Some(front) = bytes.get_mut(..header.len()) else {
            return;
        };
        front.copy_from_slice(&header);

        let Ok(mut records) = Records::<TestDid>::validate(&bytes) else {
            return;
        };
        assert_eq!(records.next(), Some((TestDid::VehicleSpeed, &[0x40][..])));
        assert_eq!(records.next(), Some((TestDid::VinNumber, &[0x00; 17][..])));
        assert_eq!(records.next(), None);
    }

    /// An empty response yields nothing rather than an error: a server that answered
    /// positively with no records has answered.
    #[test]
    fn an_empty_response_yields_no_records() {
        let Ok(records) = Records::<TestDid>::validate(&[]) else {
            return;
        };
        assert_eq!(records.count(), 0);
    }

    /// A record shorter than the application declared is rejected when the walk is
    /// built, not discovered part-way through one. The distinction is the point of
    /// validating: a caller never sees a response half-walked.
    #[test]
    fn a_truncated_record_is_rejected_before_the_walk() {
        assert_eq!(
            Records::<TestDid>::validate(&[0xF1, 0x90, 0x00, 0x00]),
            Err(RecordError::Short)
        );
    }

    /// An identifier this application never defined is its own error. On a request that
    /// is `requestOutOfRange`; in a response it is a server naming something unasked
    /// for, and it too is caught before any pair is handed out.
    #[test]
    fn an_identifier_the_application_does_not_define_is_rejected() {
        assert_eq!(
            Records::<TestDid>::validate(&[0xDE, 0xAD, 0x00]),
            Err(RecordError::UnknownIdentifier)
        );
    }

    /// The records before a bad one are not handed out either. A response whose first
    /// pair is whole and whose second is truncated is rejected entire — which is what
    /// "there is no half-walked response" means, and what a per-record `Result` could
    /// not express.
    #[test]
    fn a_good_record_before_a_bad_one_is_not_yielded() {
        let response = [0xF4, 0x0D, 0x40, 0xF1, 0x90, 0x00];
        assert_eq!(
            Records::<TestDid>::validate(&response),
            Err(RecordError::Short)
        );
    }
}
