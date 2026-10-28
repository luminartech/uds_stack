// Every unsafe block in a C binding is an assumption about the C side, so each one states
// the contract it relies on.
#![deny(clippy::undocumented_unsafe_blocks)]
// Each module relaxes `unsafe_code` for itself, so each says why.
#![deny(clippy::allow_attributes_without_reason)]
//! # `uds_c`
//!
//! A `uds_stack` UDS server for C firmware. The firmware describes its services as
//! tables of C handlers and implements a small socket and clock ABI; this crate turns
//! those into the application, the sockets and the clock a [`uds_services::uds_server!`]
//! server runs on, over [`uds_on_ip`]'s `DoIP` transport. The C contract is
//! `include/uds_c.h`, shipped with the package; every `#[repr(C)]` type here mirrors
//! one there, with the same name.
//!
//! What the firmware keeps is its own: the tables and handlers, the policy a [`Policy`]
//! states, the server's assembly and the two entry points that start and poll it.
//!
//! ## The pieces
//!
//! - [`UdsLayer`] and its tables, read in the order a [`Layers`] lists them, so an
//!   earlier layer's entry wins. [`TableApp::validate`] refuses tables the server
//!   cannot serve safely, once, at start-up, and [`Firmware::arm`] takes its proof.
//! - [`TableApp`], the application: every service `uds_services` stages, each one
//!   looked up in the layers and answered by the handler the entry names.
//! - [`uds_record_copy`], through which a handler answers: the server owns every
//!   response record and lends one to the handler being called.
//! - The DTC services over a DTC memory a layer keeps: the server reads each DTC's
//!   number, status and snapshot record and encodes the reports itself, and the memory
//!   applies the status transitions [`uds_dtc_status_after_test_result`] and its
//!   companions export.
//!   [`uds_record_copy_16_be`], [`uds_record_copy_32_be`] and [`uds_record_copy_64_be`]
//!   copy an integer big-endian, the byte order of UDS records.
//! - [`Firmware`], the server and its run future in one `static`, the one way to drive the
//!   server: each poll first makes the resets and session changes whose responses the
//!   stack has transmitted, as `uds_tcp_transmitted` reports, and resumes
//!   `ControlDTCSetting` and `CommunicationControl` state on entering a session where
//!   they are not served.
//! - [`nal`], `edge-nal` sockets over the C socket calls, and [`doip`], the entity
//!   and transport they carry.
//! - With the `time-driver` feature, `time_driver!`, `embassy-time`'s clock over
//!   `uds_now_ms`. Without it, the image links an `embassy-time` driver of its own: the
//!   security delays read `embassy_time::Instant::now`.
//!
//! ## One execution context
//!
//! Every C call this crate makes, and every entry point it exports, belongs to the one
//! execution context that polls the server, and none is reentrant. The handler being
//! called and the transition awaiting its response are each kept in a `static`, so an
//! image runs one server built on this crate.
//!
//! ## Assembly
//!
//! The firmware names its layers, states its policy and assembles the server as a
//! `static`, built in place:
//!
//! ```no_run
//! use simple_doip::LogicalAddress;
//! use simple_doip::entity::EntityAddress;
//! use simple_doip::service::{EntityConfig, TesterAddress};
//! use uds_c::{Layers, Policy, TableApp, UdsLayer, doip};
//! use uds_on_ip::profile::bench_reloads;
//! use uds_services::{
//!     Address, DiagnosticSessionType, P2StarServerMax, ServerParams, SessionTiming,
//!     Sessions, uds_server,
//! };
//!
//! unsafe extern "C" {
//!     static platform_layer: UdsLayer;
//!     static oem_layer: UdsLayer;
//! }
//!
//! // SAFETY: references to the firmware's layers, which C defines once.
//! static LAYER_LIST: [&UdsLayer; 2] = unsafe { [&platform_layer, &oem_layer] };
//! // SAFETY: the C tables are constant, and the server is polled from one context.
//! static LAYERS: Layers = unsafe { Layers::new(&LAYER_LIST) };
//!
//! const TIMING: SessionTiming = SessionTiming {
//!     p2_server_max_ms: 50,
//!     p2_star_server_max: P2StarServerMax::from_tens_of_millis(500),
//! };
//!
//! struct Ecu;
//!
//! impl Policy for Ecu {
//!     const LAYERS: &'static Layers = &LAYERS;
//!     const SESSIONS: Sessions = Sessions::NONE
//!         .with(DiagnosticSessionType::DefaultSession)
//!         .with(DiagnosticSessionType::ExtendedDiagnosticSession);
//!
//!     fn timing(_session: DiagnosticSessionType) -> SessionTiming {
//!         TIMING
//!     }
//! }
//!
//! type App = TableApp<Ecu>;
//! type Transport = doip::Transport<2048, 1>;
//!
//! uds_server! {
//!     App: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
//!          ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
//!          ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
//!     transport = Transport,
//!     peers = 1,
//!     services = EcuServices,
//!     server = EcuServer,
//!     serves = App::serves,
//! }
//!
//! # const ENTITY: EntityAddress =
//! #     match EntityAddress::new(LogicalAddress(0x4010), LogicalAddress(0xE400)) {
//! #         Ok(address) => address,
//! #         Err(_) => panic!(),
//! #     };
//! # const TESTER: TesterAddress = match TesterAddress::new(LogicalAddress(0x0E00)) {
//! #     Ok(tester) => tester,
//! #     Err(_) => panic!(),
//! # };
//! // The initializer of a `static` the firmware reaches from one context.
//! const SERVER: EcuServer = EcuServer::new(
//!     App::new(),
//!     doip::transport(ENTITY, EntityConfig::new([TESTER]), bench_reloads()),
//!     Address(0x4010),
//!     ServerParams::new(5_000, 50, 5_000, 10),
//! );
//! # fn main() {}
//! ```
//!
//! The firmware keeps the server in a [`Firmware`] `static`, naming its run future's type
//! with the nightly `type_alias_impl_trait` feature, in its own crate. At start-up it
//! checks the tables with [`TableApp::validate`] and hands the [`Validated`] proof it
//! returns to [`Firmware::arm`], whose closures start discovery with
//! [`doip::start_discovery`] and build the run future; on each pass of its loop it calls
//! [`Firmware::poll`], which builds the run future again after a transport error between
//! requests and leaves the server unarmed for good after one mid-request. `arm` fails to
//! compile for an assembly not given `serves = App::serves`, as above. The crate's
//! integration tests drive the server the same way, over a simulated C side.

#![no_std]

mod app;
mod dtc_status;
mod firmware;
mod layer;
mod op;
mod record;
mod sys;

pub mod doip;
pub mod nal;

#[cfg(feature = "time-driver")]
pub mod clock;

pub use app::{
    ACTIVE_SESSION_DID, AccessOf, Did, Policy, Rid, TableApp, TableError, UDS_DTC_ALL,
    Validated, record_len,
};
pub use dtc_status::{
    uds_dtc_status_after_test_result, uds_dtc_status_at_operation_cycle_start,
    uds_dtc_status_cleared,
};
pub use firmware::{Firmware, TableServer};
pub use layer::{
    Layers, UdsAccess, UdsCommControlFn, UdsDidEntry, UdsDidReadFn, UdsDidWriteFn,
    UdsDtcClearFn, UdsDtcGetFn, UdsDtcHooks, UdsDtcSettingFn, UdsDtcSnapshotCountFn,
    UdsDtcSnapshotItemFn, UdsKeyFn, UdsLayer, UdsRidEntry, UdsRoutineFn, UdsSecurityLevel,
    UdsSeedFn, UdsTransferBeginFn, UdsTransferBlockFn, UdsTransferFinishFn,
    UdsTransferHooks, UdsTransitionAbandonFn, UdsTransitionCheckFn, UdsTransitionExecuteFn,
    UdsTransitionHooks, UdsTransitionPrepareFn,
};
pub use record::{
    uds_record_copy, uds_record_copy_16_be, uds_record_copy_32_be, uds_record_copy_64_be,
};

/// What [`time_driver!`] expands to names, reached through this crate so the binary
/// need not depend on it.
#[cfg(feature = "time-driver")]
#[doc(hidden)]
pub mod __private {
    pub use embassy_time_driver;
}
