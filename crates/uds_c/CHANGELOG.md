# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

## [Unreleased]

### Added

- First release. `uds_c` serves UDS over `DoIP` from C firmware's tables of handlers,
  with the C contract in `include/uds_c.h`:
  - `UdsLayer` and its tables, `#[repr(C)]` mirrors of the header's, read in the order
    a `Layers` lists them.
  - `TableApp<P: Policy>`, an application implementing every `uds_services` service
    trait over the layers, built as a `const`, with `TableApp::validate` to refuse
    tables at start-up; `Policy` states the sessions, their timing and the limits.
    A DTC, communication control or download service that no layer serves is
    `serviceNotSupported` (0x11).
  - `uds_record_copy`, through which a handler answers into the record the server
    lends it, and the operation-status protocol of handlers that pend or are canceled.
    `uds_record_copy_16_be`, `uds_record_copy_32_be` and `uds_record_copy_64_be`
    copy an integer big-endian.
  - The DTC services over a DTC memory a layer keeps (`UdsDtcHooks`): the server
    reads each DTC's number, status and snapshot record and encodes the
    `ReadDTCInformation` reports itself, matches a `ClearDiagnosticInformation` group
    to a DTC before the clear handler runs, and exports the Annex D status transitions
    of `uds_services::dtc_status` to C (`uds_dtc_status_after_test_result` and its
    companions) for the memory to apply.
  - `execute_deferred`, which makes a reset or session change once its response has
    gone out.
  - `nal`, `edge-nal` sockets over the header's socket calls, and `doip`, the entity
    and transport over them with `doip::poll_run` to drive the server.
  - The `time-driver` feature and `time_driver!`, `embassy-time`'s clock over
    `uds_now_ms`.
