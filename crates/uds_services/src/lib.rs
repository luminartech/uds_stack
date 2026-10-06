//! # `uds_services`
//!
//! The point where an application meets a diagnostic stack, in both
//! directions: the interface by which a server integrates the stack, and the
//! set of requests available to a client application. Everything below deals
//! in bytes; this is the last layer that understands UDS.
//!
//! Its scope is ISO 14229-1:2020's *behaviour*: `uds_protocol` owns that
//! document's format — the bits, the bytes, and which messages are valid — and
//! this crate owns everything else in it. Clause 8.7, the server response
//! implementation rules, is the densest part of that and the reason the crate
//! exists, but it is not the boundary.
//!
//! A UDS server must do more than answer the requests it supports. Clause 8.7
//! specifies the whole validation sequence: which checks run in which order,
//! which negative response code each failure produces, and when the correct
//! answer is no response at all. This crate centralises those rules so an
//! application defines its identifiers, implements the services it supports,
//! and the rest follows.
//!
//! A client application names services in that same identifier vocabulary and
//! gets its results back in it: a `ReadDataByIdentifier` response arrives as the
//! identifier/record pairs the application declared, not as the bytes the server
//! concatenated. Negative responses come back too — the layer below explicitly
//! declines to interpret them, and this is the layer that does.
//!
//! # Status
//!
//! The server role runs end to end over any `UdsTransport` (see
//! `tests/end_to_end.rs`), and every service trait but `DataTransfer` has a pipeline
//! stage (see `tests/stages.rs`). A listed `DataTransfer` answers
//! `serviceNotSupported`. The client's
//! entry points are `todo!()` and carry the architecture element each will
//! satisfy. `tests/composition.rs` assembles a server against the whole surface.
//!
//! # Scope
//!
//! ISO 14229-1's behaviour is what is implemented; being the stack's
//! integration surface is what the crate is. Message encode/decode is
//! `uds_protocol`; session timing is `uds_session`; framing and transport are
//! the binding's. Deciding to
//! answer `requestCorrectlyReceivedResponsePending` (0x78) is this crate's,
//! because ISO 14229-2 makes that decision turn on whether the server supports
//! the service; *when* one is due remains the session layer's.
//!
//! A handler that knows how to fetch a data identifier has nothing to say
//! about IP. A binding is never a feature of this crate — it implements
//! [`UdsTransport`], and the application names it (`UDSSVC_ARCH_0003`).
//!
//! # Design constraints
//!
//! `no_std` and allocation-free. A handler writes its response into a
//! [`ResponseSink`] this crate owns rather than returning a `Vec`,
//! and no public type carries a `Vec` or a `String`. This is designed in
//! rather than deferred: the signatures that make an API alloc-free are the
//! ones callers depend on, so it cannot be retrofitted later.
//!
//! # Exhaustive matching
//!
//! Nothing here is `#[non_exhaustive]`, and that is a decision rather than an omission.
//!
//! The attribute rests on a requirement stating an *open* enumeration. No requirement in
//! this crate's set states one: every enumeration here is closed by ISO 14229 or by the
//! architecture, from [`SessionTransition`]'s four transitions of clause 10.2 Figure 7 to
//! [`DtcReportKind`]'s five response layouts of clause 12.3. `uds_session` keeps the
//! attribute on exactly the two outputs its own requirement set leaves open, and on
//! nothing else, for the same reason.
//!
//! So a caller matches exhaustively and a new variant stops their build. That is the
//! correct signal: it says a requirement changed and their handling needs revisiting.
//! `#[non_exhaustive]` would turn it into a wildcard arm that absorbs the change in
//! silence, which is the same failure as a hand-maintained variant list.

#![no_std]
#![forbid(unsafe_code)]

pub mod access;
pub use access::{Access, Levels, Sessions};

pub mod assembly;

pub mod client;
pub use client::{Answer, Client, ClientSet, Records, Response, Responses};

pub mod server;
pub use server::{Server, ServerParams};

pub mod storage;
pub use storage::{Buffers, ClientBuffers, ClientStorage, ClientStore, Storage, Store};

pub mod sink;
pub use sink::ResponseSink;

pub mod state;
pub use state::{ProtocolState, State};

/// The write vocabulary a handler needs to use a [`ResponseSink`].
///
/// [`Sink`] is re-exported because `write_all` is one of its methods: without the trait
/// in scope a handler cannot write a byte, and importing it meant declaring a dependency
/// on a crate the application otherwise never names.
///
/// [`Encode`] is the reason that matters beyond convenience. `uds_protocol`'s types
/// encode *into* a [`Sink`], and [`ResponseSink`] is one — so a handler writes
/// `DtcRecord::new(0xC0, 0x01, 0x23).encode(out)` rather than assembling the bytes by
/// hand, and the length cannot disagree with the value.
///
/// A handler discards the write's `Result`. A refused write is recorded by the sink, and
/// the pipeline answers `responseTooLong` (0x14) in its place, so it needs no handling;
/// there is no `From` conversion to a [`NegativeResponseCode`] for `?` to use either.
///
/// ```
/// use uds_services::{
///     Access, DtcRecord, DtcReportKind, DtcStatusMask, Encode, NegativeResponseCode,
///     ReadDtcInfoReportType, ReadDtcInfoSubFunction, ReadDtcInformation, ResponseSink,
///     Sessions,
/// };
///
/// struct Ecu;
///
/// impl ReadDtcInformation for Ecu {
///     const MAY_RESPOND_PENDING: bool = false;
///     const MAX_DTCS: usize = 1;
///     const REPORTS: &'static [DtcReportKind] = &[DtcReportKind::DtcList];
///
///     fn access(&self, report: ReadDtcInfoReportType) -> Option<Access> {
///         matches!(report, ReadDtcInfoReportType::ReportDtcByStatusMask)
///             .then(|| Access::new(Sessions::ALL))
///     }
///
///     async fn read_dtc_information(
///         &mut self,
///         _request: ReadDtcInfoSubFunction,
///         out: &mut ResponseSink<'_>,
///     ) -> Result<(), NegativeResponseCode> {
///         // A refusal is the sink's to record and the pipeline's to answer.
///         let _ = DtcStatusMask::from(0x09).encode(out);
///         let _ = DtcRecord::new(0xC0, 0x01, 0x23).encode(out);
///         let _ = DtcStatusMask::from(0x09).encode(out);
///         Ok(())
///     }
/// }
/// ```
pub use automotive_wire_codec::{Encode, InsufficientBuffer, Sink, WriteError};

pub mod transport;
pub use transport::{
    Address, AfterSend, Ai, Mtype, Reloads, SResult, TaType, Timestamp, TransportEvent,
    UdsTransport,
};

/// The keep-alive modes a client is built in, from `uds_session`.
///
/// Re-exported for the reason the protocol vocabulary above is: [`crate::uds_client`]'s
/// `keep_alive = ..` names one, and an application should not take a dependency on
/// `uds_session` to write it.
pub use uds_session::{FunctionalKeepAlive, KeepAliveMode, PhysicalKeepAlive};

mod select;

pub mod identifier;
pub use identifier::{DataIdentifier, RecordError, RoutineIdentifier};

pub mod services;
pub use services::{
    ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer,
    Delay, DiagnosticSessionControl, DtcReportKind, EcuReset, KeyVerdict,
    ReadDataByIdentifier, ReadDtcInformation, Received, Responded, RoutineControl,
    SecurityAccess, SecurityLevel, SecurityPolicy, ServiceSet, SessionTiming,
    SessionTransition, TesterPresent, TransferRequest, Unsettled, WriteDataByIdentifier,
};

/// The protocol vocabulary this crate's handler signatures are written in.
///
/// Re-exported so an application implementing a service trait names one crate, not two.
/// Every one of these is `uds_protocol`'s — this crate defines no sub-function, no reset
/// type and no negative response code (``UDSSVC_ARCH_0002``). They are re-exported rather
/// than re-modelled because a parallel enumeration here would be a second place for the
/// same clause to be written down, and the two would drift.
///
/// Each sub-function type is range-checked to `0x00`-`0x7F` on construction, so none of
/// them can carry `suppressPosRspMsgIndication`: that bit is the pipeline's and never
/// reaches a handler.
pub use uds_protocol::{
    CLEAR_ALL_DTCS, CommunicationControlType, CommunicationType, DiagnosticSessionType,
    DtcRecord, DtcSettingType, DtcStatusMask, FileOperationMode, FunctionalGroupIdentifier,
    NegativeResponseCode, ReadDtcInfoReportType, ReadDtcInfoSubFunction, ResetType,
    RoutineControlSubFunction, SubnetNumber, UdsServiceType,
};

/// Re-exported only so `uds_server!`'s expansion can name it; not part of the API.
#[doc(hidden)]
pub use uds_protocol::Request;

#[doc(hidden)]
pub mod sealed;

#[doc(hidden)]
pub mod pipeline;
