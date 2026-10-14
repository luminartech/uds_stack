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
  traits it implements. It declares a marker type, named by its `services` key, that
  the assembly is implemented on, so the application type may come from any crate. It
  emits a `Server<Assembly, Transport, PEERS>` alias and sizes every buffer at compile
  time from the application's declared maxima and the transport's `MAX_PDU`. It fails
  to compile when a negative response would not fit.
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
  compile. It declares a marker type, named by its `vocabulary` key, that `ClientSet`
  is implemented on, so the enumeration may come from any crate and take any number of
  clients. It emits a `Client` alias with the channel counts and keep-alive mode set.
- `Client`, the async client driver over `uds_session`'s client role:
  `read_data_by_identifier` (physical, returning the declared identifier/record
  pairs as `Records`; a reply carrying a record not asked for is malformed),
  `read_data_by_identifier_functional` (a `Responses` stream
  with one `Answer` per responding server), `diagnostic_session_control`,
  `idle_until` (waits and sends `TesterPresent` keep-alives as they fall due) and
  `close`. Negative responses are returned as values (`Response::Negative`,
  `Answer::Negative`). `ClientError` is for failures only: transport, timeout, not
  sent, closed, no channel, or a bad request. Keep-alive is configured with
  `KeepAlive` and timing policy with `ClientTiming`.
- `UdsTransport`, the async interface a transport binding implements for both
  roles: `t_data_req`, which dropped part-way has accepted its request or done
  nothing, a cancel-safe `next_event` that returns `TransportEvent`, `outbound_max`,
  `channel_timing`, `now`, and a `MAX_PDU` constant. Every client call is
  cancel-safe.
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
- `Client` calls for every service the sensor stages, and the transfer steps:
  `write_data_by_identifier`, `read_dtc_information`, `clear_diagnostic_information`,
  `security_access_seed` and `security_access_key`, `routine_control`, `ecu_reset`,
  `request_download` (narrowest address and size widths) and
  `request_download_with_format` (a caller-chosen `addressAndLengthFormatIdentifier`),
  `transfer_data`, `request_transfer_exit`, `communication_control` and
  `control_dtc_setting`. Each encodes its request through `uds_protocol`, decodes
  the reply exactly, and answers a wrong echo with `MalformedResponse::Echo`. No call
  sets the suppress-positive-response bit.
- `Client::connection_replaced` and `Replaced`: why a server's connection is not the one
  the last ask saw. `Replaced::SessionLost` is a close the protocol prescribes, after a
  positive `ecu_reset` or a positive `diagnostic_session_control` the transport marked
  as leaving, so the server's session and security are gone;
  `Replaced::SessionUnknown` is any other close, including the client's own reconnect
  to repeat an unanswered request, after which they may be intact; `Replaced::No` is
  none. A positive `ecu_reset` is reported at once and its later close not again,
  unless the server answers first; any other close once the transport has been polled
  since it arrived, `SessionLost` once where both kinds happen between asks. One ask
  takes the events the transport holds when it is made, and sends a keep-alive that
  fell due.
- `Response::map`, and `MalformedResponse::Echo` and `Decode`.
- Re-exports of `uds_protocol`'s `ReadDtcInfoResponse`, `DataFormatIdentifier`,
  `AddressAndLengthFormatIdentifier`, `DtcSnapshotRecordNumber` and
  `DtcExtDataRecordNumber`, which the new calls take or
  return.
- `ClientTiming::repeats`, a `Repeats` ceiling on a physically addressed request's
  repeats under which an expired response window is repeated: ISO 14229-2:2021 Table
  9's two (`Repeats::Twice`) by default, set by `ClientTiming::with_repeats`, read and
  changed through `Client::timing` and `Client::timing_mut`. It counts every repeat of
  the request, as Table 9 does. A failed transmission is repeated to two regardless,
  and functionally addressed requests are not under it.
- `DataTransfer` serves a download: `RequestDownload`, `TransferData` and
  `RequestTransferExit`, with the transfer held in the protocol state.
- `DataTransfer::SESSIONS`, `DataTransfer::format_supported` and `DataTransfer::access`.
- `AfterReset`, `EcuReset::confirmed`, `Stepped`, `Stopped`, and `Server::into_parts`.

### Changed

- **Breaking:** an enumeration is exhaustive only where ISO 14229 fixes its set
  (`SessionTransition`, `KeyVerdict`, `Repeats`, `Delay`, `TransferRequest`). The
  crate's own outcomes and events — `Response`, `Answer`, `TransportEvent`,
  `AfterSend`, `RecordError` and `Received` — are `#[non_exhaustive]`. Migrate: add a
  wildcard arm where one of these is matched.
- **Breaking:** `TransferRequest`'s `size` and `Client::request_download`'s `size` are
  `u64`, and a request may declare up to eight bytes each of address and size. A wider
  declaration is answered `requestOutOfRange` (0x31) before any handler is asked. A
  client's or a `DataTransfer` server's request buffer holds the 19-byte widest
  `RequestDownload`, up from 12, and a client's transport must carry it.
- **Breaking:** `DtcReportKind` is `#[non_exhaustive]` and gains `SnapshotRecords` and
  `ExtDataRecords`, sub-functions `0x04` and `0x06`, bounded in record bytes, so
  `DtcReportKind::record_len` returns `None` for them and `Some` for the rest. A client
  reads them. A server cannot answer them, and `uds_server!` fails to compile where
  `ReadDtcInformation::REPORTS` lists either.
- **Breaking:** `uds_client!` takes `routines` and `limits`, a `ClientLimits` built in a
  `const` from `ClientLimits::new()` and setters for the most identifiers per read, DTC
  records, DTC record bytes, seed, key, option record and `TransferData` block, each
  defaulting to the least. It folds the client's buffers from them as `uds_server!` folds
  a server's from its service traits; a limit added later is a new setter. `ClientSet`
  gains `type Did`, `type Routine` and `const LIMITS`. Each call refuses an input past its
  limit as `ClientError::Request` before sending. A transport whose `MAX_PDU` cannot carry
  a request of the declared limits fails to compile, naming the limit.
- **Breaking:** `DataTransfer` requires `SESSIONS`, `format_supported` and `access`.
  Migrate: declare the sessions the three services are served in, accept the formats you
  support, and return the `Access` of each memory interval (`None` for an invalid one).
- **Breaking:** `EcuReset::reset` returns `Result<AfterReset, NegativeResponseCode>`, and
  `EcuReset::confirmed` is required. Migrate: return `AfterReset::Restart` for a reset that
  restarts the server, `AfterReset::Continue` otherwise; start any post-response action in
  `confirmed`.
- **Breaking:** `Server::step` returns `Result<Stepped, _>`, which is `#[must_use]`, and
  `Server::run` returns `Result<Stopped, _>` once a restarting reset is confirmed.
  Migrate: loop on `run`, or match `Stepped::Stopped` in a `step` loop, act on it
  (rebind, delay) and step again.
- **Breaking:** `DataTransfer::SUPPORTS_UPLOAD` is removed, since no upload is served.
  Migrate: delete it from the implementation.
- **Breaking:** `Responded` carries an `Effect` in place of a session and is hidden from the
  documentation; applications never built or read it.
- **Breaking:** `uds_server!` fails to compile where the transport cannot carry a
  `TransferData` block of `MAX_BLOCK_LENGTH`, which an assembly may have compiled before.
  Migrate: raise the transport's message size or lower `MAX_BLOCK_LENGTH`.

### Known limitations

- A server serves one tester: `uds_server!` accepts only `peers = 1`, and any other
  count fails to compile.
- Upload and file transfer (0x35, 0x38) are not served: a server that lists `DataTransfer`
  answers `serviceNotSupported` to them.
- The client sends `TesterPresent` only as its keep-alive. The transfer calls are
  single exchanges: the block sequence and any orchestration above them are the caller's.
- An `Err` from `Server::step` or `Server::run` is terminal for that server
  instance, and neither future is cancel-safe. Drop `step` only between calls.
