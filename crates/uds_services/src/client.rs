//! The client role.
//!
//! ``UDSSVC_ARCH_0020`` — a client issues requests over the application's own
//! identifiers, and ``UDSSVC_ARCH_0024`` makes that the same vocabulary a server's
//! handlers are written against. A client implements no service trait.
//!
//! ``UDSSVC_ARCH_0021`` — a negative response is interpreted here; nothing below reads
//! a response code.
//!
//! The exchanges themselves run in `client::driver` over `uds_session`'s client role and
//! [`UdsTransport`], as [`crate::Server`]'s do; `client::encode` holds the sans-io halves
//! (``UDSSVC_ARCH_0028``).
//!
//! **The storage shape mirrors `uds_session`'s.** That crate splits channels by kind:
//! `PhysicalSlot` carries no responder table where `FunctionalSlot<R>` does, and a
//! physical channel's `tS3_Client` is an argument of `open_physical_channel` in physical
//! keep-alive and absent from it in functional keep-alive.
//! Mirroring the split costs a third const parameter and buys the same thing it buys
//! them — a physical-only client pays nothing for responder tables, and a functional
//! channel cannot be given a session reload it has no use for.

mod driver;
mod encode;

use crate::storage::{ClientBuffers, ClientStorage};
use crate::{
    ClientTransport, DataIdentifier, DiagnosticSessionType, RecordError, SessionTiming,
    UdsServiceType,
};
use driver::{Answered, Book, Exchange};

#[cfg(doc)]
use crate::UdsTransport;
use uds_protocol::NegativeResponseCode;
use uds_session::{
    Address, ChannelAddressing, ChannelParameter, ChannelParams, ClientTx,
    ExpectedResponses, FunctionalKeepAlive, KeepAliveMode, PhysicalChannelId,
    PhysicalKeepAlive, Rejection, Reloads, Timestamp,
};

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

/// Why a call produced no response.
///
/// ``UDSSVC_ARCH_0023`` — a timeout is a fault, as are a request that could not be sent
/// and a connection that closed under it. A negative response is not here: it is a
/// response, [`Response::Negative`] or [`Answer::Negative`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientError<E> {
    /// The transport failed; see [`UdsTransport::Error`].
    Transport(E),
    /// No response came within `tP_Client`, after ISO 14229-2:2021 9.7 Table 9's two
    /// repeats.
    Timeout,
    /// The request's last transmission was confirmed failed, after
    /// ISO 14229-2:2021 Table 9's repeats, or the session layer refused the request
    /// outright, which is not repeated.
    NotSent,
    /// The connection to the server closed before it answered.
    Closed,
    /// Every channel slot was in use, and none was idle enough to withdraw.
    NoChannel,
    /// The request named no identifier, or more than the client's
    /// `max_dids_per_request`; nothing was sent.
    Request,
}

impl<E: core::fmt::Debug> core::fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "the transport failed: {error:?}"),
            Self::Timeout => f.write_str("no response within the response window"),
            Self::NotSent => f.write_str("the request could not be sent"),
            Self::Closed => f.write_str("the connection closed before the response"),
            Self::NoChannel => f.write_str("no channel slot is free"),
            Self::Request => f.write_str("the request names no identifier, or too many"),
        }
    }
}

impl<E: core::fmt::Debug> core::error::Error for ClientError<E> {}

/// A keep-alive mode a [`Client`] can be built in: [`FunctionalKeepAlive`] or
/// [`PhysicalKeepAlive`], sealed to those two.
///
/// It says what a [`KeepAlive`] holds beside the mode and how a physical channel is
/// opened in it, which is all the two modes differ in at this layer.
pub trait ClientKeepAlive: KeepAliveMode + crate::sealed::Sealed + Sized {
    /// What a [`KeepAlive`] in this mode holds beside the mode itself.
    #[doc(hidden)]
    type Setting: Copy + core::fmt::Debug;

    /// Open a physical channel in this mode.
    #[doc(hidden)]
    fn open_physical<const PHYS: usize, const FUNC: usize, const R: usize>(
        session: &mut uds_session::Client<Self, PHYS, FUNC, R>,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
        setting: Self::Setting,
    ) -> Result<PhysicalChannelId, Rejection>;

    /// The functional address a keep-alive is sent to, in the mode that has one.
    #[doc(hidden)]
    fn group(setting: Self::Setting) -> Option<Address>;
}

impl crate::sealed::Sealed for FunctionalKeepAlive {}

impl ClientKeepAlive for FunctionalKeepAlive {
    type Setting = Address;

    fn open_physical<const PHYS: usize, const FUNC: usize, const R: usize>(
        session: &mut uds_session::Client<Self, PHYS, FUNC, R>,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
        _group: Address,
    ) -> Result<PhysicalChannelId, Rejection> {
        let uds_session::Finished { outcome, rest: _ } = session
            .open_physical_channel(now, addressing, params)
            .finish();
        outcome
    }

    fn group(group: Address) -> Option<Address> {
        Some(group)
    }
}

impl crate::sealed::Sealed for PhysicalKeepAlive {}

impl ClientKeepAlive for PhysicalKeepAlive {
    type Setting = u32;

    fn open_physical<const PHYS: usize, const FUNC: usize, const R: usize>(
        session: &mut uds_session::Client<Self, PHYS, FUNC, R>,
        now: Timestamp,
        addressing: ChannelAddressing,
        params: ChannelParams,
        s3_client: u32,
    ) -> Result<PhysicalChannelId, Rejection> {
        let uds_session::Finished { outcome, rest: _ } = session
            .open_physical_channel(now, addressing, params, s3_client)
            .finish();
        outcome
    }

    fn group(_s3_client: u32) -> Option<Address> {
        None
    }
}

/// How a [`Client`] keeps a server's non-default session alive, fixed at creation.
///
/// ISO 14229-2:2021 9.5 Tables 5 and 6 — a `TesterPresent` every `tS3_Client` while a
/// server is out of its default session. This client sends none to a server a request is
/// awaiting a response from, the request keeping that server alive itself, and none
/// while a functional window is open, since that awaits every server; one falling due
/// meanwhile goes once it may. Any other goes out on time, mid-exchange. A failed one
/// goes again one `tS3_Client` later; a functional one is also repeated up to
/// ISO 14229-2:2021 9.7 Table 9's two times first. The mode is the type parameter, so a
/// client is built in exactly one.
#[derive(Debug)]
pub struct KeepAlive<K: ClientKeepAlive> {
    /// Never dropped, only moved into the session layer: a `const fn` cannot destructure
    /// a value whose generic part might have a destructor.
    mode: core::mem::ManuallyDrop<K>,
    setting: K::Setting,
}

impl KeepAlive<PhysicalKeepAlive> {
    /// One physically addressed `TesterPresent` per server in a non-default session.
    ///
    /// # Arguments
    ///
    /// * `s3_client` - `tS3_Client`, in milliseconds, for every physical channel.
    #[must_use]
    pub const fn physical(s3_client: u32) -> Self {
        Self {
            mode: core::mem::ManuallyDrop::new(PhysicalKeepAlive),
            setting: s3_client,
        }
    }
}

impl KeepAlive<FunctionalKeepAlive> {
    /// One functionally addressed `TesterPresent` for every server.
    ///
    /// # Arguments
    ///
    /// * `s3_client` - `tS3_Client`, in milliseconds.
    /// * `group` - the functional address it is sent to. ISO 14229-2 names none; it is
    ///   the bus's (`0xE400` on `DoIP` by convention).
    #[must_use]
    pub const fn functional(s3_client: u32, group: Address) -> Self {
        Self {
            mode: core::mem::ManuallyDrop::new(FunctionalKeepAlive::new(s3_client)),
            setting: group,
        }
    }
}

/// The client's timing policy: what ISO 14229-2:2021 9.2 Tables 3 and 4 leave to the
/// client rather than the transport, in milliseconds.
///
/// Built with [`Self::new`]; it may gain fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ClientTiming {
    /// `tP3_Client_Phys`, the minimum time after a physically addressed request that
    /// expects no response before the next on the channel (ISO 14229-2:2021 10.3,
    /// ``UDSS_LLR_0169``). A request that would break it is held until it may go, never
    /// reported.
    pub physical_spacing: u32,
    /// `tP3_Client_Func`, the minimum time after a functionally addressed request before
    /// the next on the channel (``UDSS_LLR_0170``).
    pub functional_spacing: u32,
    /// `ΔtP6`, the network's worst-case round trip plus margin (ISO 14229-2:2021 REQ 5.21,
    /// Formula (2)). Added to the `P2Server_max` and `P2*Server_max` a server advertises to
    /// give the waits for its response: ISO 14229-2:2021 Table 4's minimum for
    /// `tP6_Client`, and more than its minimum for `tP6*_Client`, which adds only
    /// `ΔtP6_Response` and leaves a longer wait to the client. Vehicle-specific; the
    /// standard gives no default.
    pub network_delay: u32,
}

impl ClientTiming {
    /// A timing policy.
    ///
    /// # Arguments
    ///
    /// * `physical_spacing` - see [`Self::physical_spacing`].
    /// * `functional_spacing` - see [`Self::functional_spacing`].
    /// * `network_delay` - see [`Self::network_delay`].
    #[must_use]
    pub const fn new(
        physical_spacing: u32,
        functional_spacing: u32,
        network_delay: u32,
    ) -> Self {
        Self {
            physical_spacing,
            functional_spacing,
            network_delay,
        }
    }
}

/// Why a response could not be read.
///
/// The server answered and the transport delivered what it sent, so this is a
/// disagreement between two applications about the bytes, not a link fault: see
/// [`Response::Malformed`] and [`Answer::Malformed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MalformedResponse {
    /// A record the application's [`DataIdentifier::split_record`] rejected.
    Record(RecordError),
    /// The response named an identifier this application does not define, which a
    /// server never asked for it can only have made up.
    UnknownIdentifier,
    /// The response was longer than the client's buffer, so it was not read at all.
    ///
    /// The buffer is folded from this vocabulary's declared maxima, so only a server
    /// sending more than the vocabulary declares can overrun it. Its records are not
    /// walked: a cut at a record boundary would read as a shorter, valid answer.
    Overlong {
        /// How long the response was, where the transport knew.
        declared: Option<usize>,
    },
    /// The response was shorter than its service's fixed fields.
    Short,
}

impl From<RecordError> for MalformedResponse {
    fn from(error: RecordError) -> Self {
        Self::Record(error)
    }
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
    Malformed(MalformedResponse),
    /// The exchange completed and nothing came back, as a request with the suppress bit
    /// set expects. Distinct from a timeout, because a timeout is a fault and suppression
    /// is not.
    ///
    /// No call returns this yet: none sets the suppress bit, and a silent functional
    /// window ends a [`Responses`] instead.
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
        error: MalformedResponse,
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
/// until the next is taken. ``UDSSVC_ARCH_0017``'s reasoning — an owned sequence
/// allocates per response. An inherent `async fn` rather than a `Stream`, because a
/// `Stream` item cannot borrow the receive buffer.
///
/// The request goes out on the first [`Self::next`]. The request in progress is recorded
/// in the [`Client`], not here, so dropping this part-way is safe: the client's next call
/// drains what is left of the window before anything else.
#[derive(Debug)]
#[must_use = "an undrained sequence discards the answers the request produced"]
pub struct Responses<
    'c,
    C: ClientSet,
    T: ClientTransport,
    K: ClientKeepAlive,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize = 0,
> {
    client: &'c mut Client<C, T, K, PHYS, FUNC, R>,
    /// Why the request could not be made, reported by the first [`Self::next`].
    refused: Option<ClientError<T::Error>>,
}

impl<
    C: ClientSet,
    T: ClientTransport,
    K: ClientKeepAlive,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize,
> Responses<'_, C, T, K, PHYS, FUNC, R>
{
    /// The next answer, or `None` when the response window has closed.
    ///
    /// The first call sends the request. The window closes one response timeout after
    /// the last answer, or after the request's confirmation where none came; a
    /// response-pending message holds it open and is not an answer.
    ///
    /// # Errors
    ///
    /// Each an end of the sequence, so the call after it returns `None`:
    ///
    /// - [`ClientError::Transport`] where the transport failed;
    /// - [`ClientError::NotSent`] where the request's last transmission failed;
    /// - [`ClientError::NoChannel`] or [`ClientError::Request`] where the request could
    ///   not be made at all.
    ///
    /// A server's connection closing ends nothing: the window awaits every server, and
    /// closes at its own timeout.
    ///
    /// # Cancel safety
    ///
    /// As [`Client::read_data_by_identifier`]'s. Dropping this `Responses` part-way is
    /// safe too: the next call on the client drains what is left of the window first.
    pub async fn next(&mut self) -> Option<Result<Answer<'_, C>, ClientError<T::Error>>> {
        if let Some(error) = self.refused.take() {
            return Some(Err(error));
        }
        if let Err(error) = self.client.drain_window().await {
            self.client.exchange = None;
            return Some(Err(error));
        }
        let answered = match self.client.advance().await? {
            Ok(answered) => answered,
            Err(error) => return Some(Err(error)),
        };
        let message = answered_bytes(&mut self.client.store, &answered);
        let answer = encode::final_response(message, answered.arrived);
        Some(Ok(encode::answer(answered.from, answer)))
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
/// framing error is one [`MalformedResponse`] reported once — as [`Answer::Malformed`] or
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
    /// [`MalformedResponse`] where `response` holds no record (ISO 14229-1:2020
    /// Table 188), an identifier is not one this application defines, a record is
    /// shorter than it declared or invalid, or bytes trail the last whole record.
    pub(crate) fn validate(response: &'d [u8]) -> Result<Self, MalformedResponse> {
        if response.is_empty() {
            return Err(MalformedResponse::Short);
        }
        let mut rest = response;
        while !rest.is_empty() {
            let Some((identifier, tail)) = rest.split_first_chunk::<2>() else {
                return Err(MalformedResponse::Short);
            };
            let Some(did) = D::from_u16(u16::from_be_bytes(*identifier)) else {
                return Err(MalformedResponse::UnknownIdentifier);
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
    T: ClientTransport,
    K: ClientKeepAlive,
    const PHYS: usize,
    const FUNC: usize,
    const R: usize = 0,
> {
    session: uds_session::Client<K, PHYS, FUNC, R>,
    transport: T,
    store: C::Store,
    /// This client's own `S_AI[SA]`.
    tester: Address,
    keep_alive: K::Setting,
    timing: ClientTiming,
    book: Book<PHYS, FUNC>,
    /// The request in progress, if any; see the `driver` module.
    exchange: Option<Exchange>,
    /// A functional window a dropped [`Responses`] left open, drained before anything
    /// else.
    draining: Option<Exchange>,
}

impl<
    C: ClientSet,
    T: ClientTransport,
    K: ClientKeepAlive,
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
    /// building its own session. Channels are opened as calls need them, one per server
    /// and kind, with the reloads [`UdsTransport::channel_timing`] dictates. No channel
    /// handle crosses this API, so two clients cannot confuse each other's, and
    /// `uds_session`'s handle tag is left at its default.
    ///
    /// # Arguments
    ///
    /// * `transport` - what the client's requests go over.
    /// * `tester` - this client's own `S_AI[SA]`, the source of every request.
    /// * `keep_alive` - the keep-alive mode and its `tS3_Client`; see [`KeepAlive`].
    /// * `timing` - the client's own timing policy; see [`ClientTiming`].
    pub const fn new(
        transport: T,
        tester: Address,
        keep_alive: KeepAlive<K>,
        timing: ClientTiming,
    ) -> Self {
        let KeepAlive { mode, setting } = keep_alive;
        Self {
            session: uds_session::Client::new(
                [uds_session::PhysicalSlot::EMPTY; PHYS],
                [uds_session::FunctionalSlot::EMPTY; FUNC],
                core::mem::ManuallyDrop::into_inner(mode),
            ),
            transport,
            store: <C::Store as ClientStorage>::EMPTY,
            tester,
            keep_alive: setting,
            timing,
            book: Book::EMPTY,
            exchange: None,
            draining: None,
        }
    }

    /// The transport this client sends over.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// Read one or more data identifiers from one server.
    ///
    /// The response comes back as [`Records`], the identifier/record pairs it carries,
    /// not as the undivided bytes: the application already declared each record's
    /// structure through [`DataIdentifier::split_record`], so re-walking the response by
    /// hand would be the caller redoing work this crate can do.
    ///
    /// A response-pending message (`0x78`) extends the wait to the enhanced response
    /// window, which [`Self::diagnostic_session_control`] sets from the server's own
    /// `P2*Server_max` and [`ClientTiming::network_delay`]. A response longer than the
    /// client's buffer is [`Response::Malformed`] with [`MalformedResponse::Overlong`].
    ///
    /// # Arguments
    ///
    /// * `target` - the server's `S_AI[TA]`.
    /// * `identifiers` - what to read, at least one and at most the client's
    ///   `max_dids_per_request` (see [`crate::uds_client`]).
    ///
    /// # Errors
    ///
    /// A negative response is **not** an error — it arrives as [`Response::Negative`].
    ///
    /// - [`ClientError::Request`] where `identifiers` is empty or too long; nothing is
    ///   sent.
    /// - [`ClientError::Timeout`] or [`ClientError::NotSent`] where ISO 14229-2:2021 9.7
    ///   ISO 14229-2:2021 Table 9's repeats were spent.
    /// - [`ClientError::Closed`] where the connection closed first.
    /// - [`ClientError::NoChannel`] where no channel could be opened to `target`.
    /// - [`ClientError::Transport`] where the transport failed.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at every await but the transport's own
    /// [`UdsTransport::t_data_req`]: the exchange is recorded in the client, and the next
    /// call resets its channel. A future dropped inside `t_data_req` may leave a
    /// transmission whose confirmation never comes, which nothing but the transport can
    /// clear.
    pub async fn read_data_by_identifier(
        &mut self,
        target: Address,
        identifiers: &[C],
    ) -> Result<Response<Records<'_, C>>, ClientError<T::Error>> {
        let answered = self
            .exchange_with(
                target,
                |request| encode::read_data_by_identifier(request, identifiers),
                None,
            )
            .await?;
        let message = answered_bytes(&mut self.store, &answered);
        Ok(encode::records(encode::final_response(
            message,
            answered.arrived,
        )))
    }

    /// Change one server's diagnostic session.
    ///
    /// ISO 14229-1:2020 10.2 — the positive response carries the session's
    /// [`SessionTiming`], read in Table 29's units. Its `P2Server_max` and
    /// `P2*Server_max`, each plus [`ClientTiming::network_delay`], become the waits this
    /// client allows for a response from `target` and after a response-pending message
    /// from it, each where it is longer than the transport's own. A positive response
    /// echoing another session is a late reply to an earlier change and is not taken for
    /// the answer. Entering a non-default session starts the keep-alive the
    /// client was built with. Returning to the default session ends a physical
    /// keep-alive for `target`, and the functional keep-alive once no server this client
    /// put in a non-default session remains in one.
    ///
    /// # Arguments
    ///
    /// * `target` - the server's `S_AI[TA]`.
    /// * `session` - the session to enter; see [`DiagnosticSessionType`].
    ///
    /// # Errors
    ///
    /// As [`Self::read_data_by_identifier`]'s, but for [`ClientError::Request`], which
    /// this cannot return. A negative response is **not** an error.
    ///
    /// # Cancel safety
    ///
    /// As [`Self::read_data_by_identifier`]'s.
    pub async fn diagnostic_session_control(
        &mut self,
        target: Address,
        session: DiagnosticSessionType,
    ) -> Result<Response<SessionTiming>, ClientError<T::Error>> {
        let selection = crate::services::session::selection_of(session);
        let exchanged = self
            .exchange_with(
                target,
                |request| encode::diagnostic_session_control(request, session),
                Some(selection),
            )
            .await;
        let answered = match exchanged {
            Ok(answered) => answered,
            Err(error) => {
                self.settle_functional_keep_alive();
                return Err(error);
            }
        };
        let message = answered_bytes(&mut self.store, &answered);
        let positive =
            encode::is_positive(UdsServiceType::DiagnosticSessionControl, message);
        let response =
            encode::session_timing(encode::final_response(message, answered.arrived));
        if positive {
            let timing = match response {
                Response::Positive(timing) => Some(timing),
                _ => None,
            };
            self.entered(target, selection, timing);
        }
        self.settle_functional_keep_alive();
        Ok(response)
    }

    /// Wait until `until`, sending every keep-alive that falls due meanwhile.
    ///
    /// The client's `sleep`: a keep-alive is only ever sent from inside a call, so an
    /// application with a server in a non-default session waits here between requests.
    /// One that does not still has an overdue keep-alive sent during its next call, since
    /// the session layer holds it until then: a physical one as soon as that call first
    /// waits on the transport (to the server the call addresses, the request itself
    /// stands in for it), the functional one once the call's exchange has ended. What
    /// arrives meanwhile is handed to the session layer and otherwise discarded.
    ///
    /// # Arguments
    ///
    /// * `until` - when to return, on the transport's clock ([`UdsTransport::now`]).
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] where the transport failed.
    ///
    /// # Cancel safety
    ///
    /// As [`Self::read_data_by_identifier`]'s.
    pub async fn idle_until(
        &mut self,
        until: Timestamp,
    ) -> Result<(), ClientError<T::Error>> {
        self.retire();
        self.drain_window().await?;
        loop {
            self.send_owed().await.result?;
            if self.transport.now().has_reached(until) {
                return Ok(());
            }
            self.pump(None, Some(until)).await?;
        }
    }

    /// End the session: close the transport's connection, gracefully.
    ///
    /// A call dropped part-way is settled first, and every request still awaiting its
    /// confirmation is confirmed, failed, before this returns, so the next call reads
    /// nothing of them. No `TesterPresent` follows: every keep-alive ends with the
    /// session (``UDSS_LLR_0184``). The next call opens a connection again, where the
    /// transport has connections to open.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] where closing or reading the confirmations fails; the
    /// connection is closed either way, and the confirmations are read even where closing
    /// failed.
    ///
    /// # Cancel safety
    ///
    /// As [`Self::read_data_by_identifier`]'s.
    pub async fn close(&mut self) -> Result<(), ClientError<T::Error>> {
        self.retire();
        if let Some(window) = self.draining.take() {
            self.abandon(window.ai);
        }
        self.end_keep_alives();
        let closed = self.transport.close().await;
        while self.book.unconfirmed() {
            self.pump(None, None).await?;
        }
        closed.map_err(ClientError::Transport)
    }

    /// Read one or more data identifiers from every server on a functional address.
    ///
    /// ``UDSSVC_ARCH_0022`` — a functional request reaches every server, so zero or
    /// more may answer and there is no single response to return. The answers come back
    /// as a lending sequence: this is the only way to obtain a [`Responses`], and
    /// draining it is how each server's answer is read. Nothing is sent until its first
    /// [`Responses::next`], which is also where a refusal surfaces. A negative response is
    /// [`Answer::Negative`], which is not an error.
    ///
    /// # Arguments
    ///
    /// * `target` - the functional address, `S_AI[TA]`.
    /// * `identifiers` - as for [`Self::read_data_by_identifier`].
    pub fn read_data_by_identifier_functional(
        &mut self,
        target: Address,
        identifiers: &[C],
    ) -> Responses<'_, C, T, K, PHYS, FUNC, R> {
        self.retire();
        let ai = self
            .addressing(target)
            .with_ta_type(uds_session::TaType::Functional);
        let ClientBuffers { request, .. } = self.store.split();
        let refused = match encode::read_data_by_identifier(request, identifiers) {
            Some(len) => {
                let sid = request.first().copied().unwrap_or_default();
                let class = ClientTx::Request {
                    expected: ExpectedResponses::Unknown,
                    repeat: false,
                    session: None,
                };
                self.exchange = Some(Exchange::new(ai, len, sid, None, class));
                None
            }
            None => Some(ClientError::Request),
        };
        Responses {
            client: self,
            refused,
        }
    }

    /// Run one physical exchange with `target`: settle what an earlier call left,
    /// encode the request with `encode`, and wait for its answer.
    async fn exchange_with(
        &mut self,
        target: Address,
        encode: impl FnOnce(&mut [u8]) -> Option<usize>,
        session: Option<uds_session::SessionSelection>,
    ) -> Result<Answered, ClientError<T::Error>> {
        self.retire();
        self.drain_window().await?;
        let ClientBuffers { request, .. } = self.store.split();
        let len = encode(request).ok_or(ClientError::Request)?;
        let sid = request.first().copied().unwrap_or_default();
        let echo = session.and_then(|_| request.get(1).copied());
        let ai = self
            .addressing(target)
            .with_ta_type(uds_session::TaType::Physical);
        let class = ClientTx::Request {
            expected: ExpectedResponses::Exactly(core::num::NonZeroU16::MIN),
            repeat: false,
            session,
        };
        self.exchange = Some(Exchange::new(ai, len, sid, echo, class));
        let answered = self.advance().await.unwrap_or(Err(ClientError::Timeout))?;
        // The answer stands: a keep-alive the transport fails to take now stays owed,
        // and the failure meets the next call.
        let _ = self.send_owed().await;
        Ok(answered)
    }

    /// Record that `target` answered a session change positively, as the session layer
    /// classified it: its keep-alive standing, and, where its `timing` could be read,
    /// `P2Server_max` and `P2*Server_max` plus the network delay as the response
    /// reloads, each where it is longer than the transport's (ISO 14229-2:2021 9.2
    /// Table 4, `tP6_Client` and `tP6*_Client`).
    fn entered(
        &mut self,
        target: Address,
        selection: uds_session::SessionSelection,
        timing: Option<SessionTiming>,
    ) {
        let now = self.transport.now();
        let floor = self.transport.channel_timing();
        let delay = self.timing.network_delay;
        if let Some(o) = self.book.physical_to(target) {
            o.in_session = selection == uds_session::SessionSelection::NonDefault;
            if let Some(timing) = timing {
                let wait =
                    |server: u32, floor: u32| server.saturating_add(delay).max(floor);
                let parameter = ChannelParameter::Reloads(Reloads {
                    default_reload: wait(timing.p2_server_max(), floor.default_reload),
                    enhanced_reload: wait(
                        timing.p2_star_server_max(),
                        floor.enhanced_reload,
                    ),
                });
                let _ = self
                    .session
                    .set_physical_parameter(now, o.id, parameter)
                    .finish();
            }
        }
    }
}

/// The bytes `answered` occupies in the response buffer.
fn answered_bytes<'s, S: ClientStorage>(store: &'s mut S, answered: &Answered) -> &'s [u8] {
    let ClientBuffers { response, .. } = store.split();
    response.get(answered.range.clone()).map_or(&[], |m| m)
}

#[cfg(test)]
mod tests {
    use super::{Answer, MalformedResponse, Records, Response};
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
    /// layer that interprets it: `uds_session` indicates the message and `UdsTransport`
    /// carries its bytes, and neither reads a response code.
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
            error: MalformedResponse::Short,
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

    /// ISO 14229-1:2020 Table 188 — a positive response carries at least one identifier
    /// and its record, so one with none is too short rather than an empty answer.
    #[test]
    fn an_empty_response_is_short() {
        assert_eq!(
            Records::<TestDid>::validate(&[]),
            Err(MalformedResponse::Short)
        );
    }

    /// A record shorter than the application declared is rejected when the walk is
    /// built, not discovered part-way through one. The distinction is the point of
    /// validating: a caller never sees a response half-walked.
    #[test]
    fn a_truncated_record_is_rejected_before_the_walk() {
        assert_eq!(
            Records::<TestDid>::validate(&[0xF1, 0x90, 0x00, 0x00]),
            Err(MalformedResponse::Record(RecordError::Short))
        );
    }

    /// An identifier this application never defined is its own error. On a request that
    /// is `requestOutOfRange`; in a response it is a server naming something unasked
    /// for, and it too is caught before any pair is handed out.
    #[test]
    fn an_identifier_the_application_does_not_define_is_rejected() {
        assert_eq!(
            Records::<TestDid>::validate(&[0xDE, 0xAD, 0x00]),
            Err(MalformedResponse::UnknownIdentifier)
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
            Err(MalformedResponse::Record(RecordError::Short))
        );
    }
}
