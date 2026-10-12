# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

## [Unreleased]

First release. `uds_services` implements the behaviour of ISO 14229-1:2020, meaning
everything in that document except the message format, which belongs to
`uds_protocol`. It provides typed UDS service dispatch over identifiers the
application defines, and the clause 8.7 server response rules. It is released in
lockstep with the rest of the `uds_stack` workspace (`uds_protocol`, `uds_session`,
`simple_doip` and `uds_on_ip`), all at one shared version.

### Added

- `uds_server!`, which builds a server from an application type and the service
  traits it implements. It emits a `Server<App, Transport, PEERS>` alias and sizes
  every buffer at compile time from the application's declared maxima and the
  transport's `MAX_PDU`. It fails to compile when a negative response would not fit.
- `Server`, the async server driver. It owns a `uds_session::Server` and the
  transport, applies the clause 8.7 validation sequence, and is driven by `step()`
  or `run()`. Requests for services the application does not implement, or for
  identifiers outside its supported set, are answered automatically
  (`serviceNotSupported`, `requestOutOfRange`). Session and security preconditions
  produce the correct NRC, and an unsupported functionally addressed request gets
  no response. `requestCorrectlyReceivedResponsePending` (0x78) is sent for
  services that declare `MAY_RESPOND_PENDING`. `Server::new` is `const`, so a
  server can be built in place in a `static`.
- Service traits for the server role: `DiagnosticSessionControl` (0x10),
  `EcuReset` (0x11), `ClearDiagnosticInformation` (0x14), `ReadDtcInformation`
  (0x19), `ReadDataByIdentifier` (0x22), `SecurityAccess` (0x27),
  `CommunicationControl` (0x28), `WriteDataByIdentifier` (0x2E), `RoutineControl`
  (0x31), `TesterPresent` (0x3E) and `ControlDtcSetting` (0x85). For
  `SecurityAccess` the crate runs the Annex I seed/key sequence, attempt counting
  and delay under a `SecurityPolicy`. The application supplies only the
  cryptography and non-volatile storage. Sub-function access is declared through
  `Access`, `Sessions` and `Levels`. Handlers are `async fn` and write responses
  into a `ResponseSink`.
- `uds_client!`, which builds a client from the same identifier enumeration the
  server's handlers use. Changing that enumeration on only one side fails to
  compile. It emits a `Client` alias with the channel counts and keep-alive mode
  set.
- `Client`, the async client driver over `uds_session`'s client role:
  `read_data_by_identifier` (physical, returning the declared identifier/record
  pairs as `Records`), `read_data_by_identifier_functional` (a `Responses` stream
  with one `Answer` per responding server), `diagnostic_session_control`,
  `idle_until` (waits and sends `TesterPresent` keep-alives as they fall due) and
  `close`. Negative responses are returned as values (`Response::Negative`,
  `Answer::Negative`). `ClientError` is for failures only: transport, timeout, not
  sent, closed, no channel, or a bad request. Keep-alive is configured with
  `KeepAlive` and timing policy with `ClientTiming`.
- `UdsTransport`, the async interface a transport binding implements for both
  roles: `t_data_req`, a cancel-safe `next_event` that returns `TransportEvent`,
  `outbound_max`, `channel_timing`, `now`, and a `MAX_PDU` constant.
  `ClientTransport` extends it with `close` for the client role. The crate never
  depends on a binding; `uds_on_ip` provides the `DoIP` one.
- `DataIdentifier` and `RoutineIdentifier`, the traits an application's identifier
  enumerations implement. `DataIdentifier::split_record` lets a client split a
  response into records.
- Re-exports of the `uds_protocol` types used in handler signatures, of
  `automotive-wire-codec`'s `Encode`, `Sink`, `WriteError` and `InsufficientBuffer`,
  and of `uds_session`'s keep-alive modes. An application implementing a service
  names only this crate.
- `no_std` and allocation-free in every configuration, with no Cargo features.
  Responses are written into caller-owned storage, and no public type carries a
  `Vec` or `String`. No async runtime is named. `unsafe` is forbidden. MSRV 1.91.

### Known limitations

- A server serves one tester: `uds_server!` accepts only `peers = 1`, and any other
  count fails to compile.
- The upload/download services (0x34–0x38) are not served yet. The `DataTransfer`
  trait exists, but a server that lists it answers `serviceNotSupported`.
- The client supports `ReadDataByIdentifier` and `DiagnosticSessionControl`, plus
  `TesterPresent` keep-alives.
- An `Err` from `Server::step` or `Server::run` is terminal for that server
  instance, and neither future is cancel-safe. Drop `step` only between calls.
