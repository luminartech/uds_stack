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
    tables at start-up, whose `Validated<App>` proof `Firmware::arm` takes for a server
    over that application; `Policy` states the sessions, their timing and the limits.
    A DTC, communication control or download service that no layer serves is
    `serviceNotSupported` (0x11), the assembly given `serves = App::serves`, which
    `Firmware::arm` requires at compile time.
  - Resets and session changes through every layer's `UdsTransitionHooks`: check and
    prepare before the positive response, execute after it, and abandon for a change
    prepared but not made; each hook is told the active session.
  - `uds_record_copy`, through which a handler answers into the record the server
    lends it, and the operation-status protocol of handlers that pend or are canceled.
    `uds_record_copy_16_be`, `uds_record_copy_32_be` and `uds_record_copy_64_be`
    copy an integer big-endian.
  - The DTC services over the one DTC memory a layer keeps (`UdsDtcHooks`): the server
    reads each DTC's number, status and snapshot record and encodes the
    `ReadDTCInformation` reports itself, against the availability mask
    `Policy::DTC_AVAILABILITY` declares, matches a `ClearDiagnosticInformation` group
    to a DTC before the memory's clear handler runs, and exports the Annex D status transitions
    of `uds_services::dtc_status` to C (`uds_dtc_status_after_test_result` and its
    companions) for the memory to apply.
  - `Firmware`, the server and its run future in one `static`, the one way to drive the
    server from one execution context: `arm` at start-up, then `poll` on each pass of
    its loop, both `unsafe` on that promise; `arm` again where a transport error
    stopped it, `doip::start_discovery` keeping the discovery started. `poll` makes
    the transitions through the layers of the server's own policy. Each poll makes a
    reset or session change once its response is transmitted, as the firmware's
    `uds_tcp_transmitted` reports for the connection's stream, and on
    entering a session where `ControlDTCSetting` and `CommunicationControl` are not
    served, or after a reset that restarts the server whose execute returns, resumes
    their state through their handlers (ISO 14229-1:2020 10.2), once. A reset runs before the default session it returns to.
    `enableRapidPowerShutDown` and `disableRapidPowerShutDown` keep the server running.
  - `nal`, `edge-nal` sockets over the header's socket calls, and `doip`, the entity
    and transport over them. The entity confirms a response once the firmware's
    `uds_tcp_transmitted` reports it transmitted.
  - The `time-driver` feature and `time_driver!`, `embassy-time`'s clock over
    `uds_now_ms`.
