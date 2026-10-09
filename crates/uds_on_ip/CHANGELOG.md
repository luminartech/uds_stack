# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

## [Unreleased]

First release. `uds_on_ip` implements ISO 14229-5:2022 (`UDSonIP`), which binds UDS to
a `DoIP` transport. It covers clause 8, the application profile, and clause 11, the
mapping of `T_PDU` service primitives onto `DoIP`. It provides `uds_services`'s
transport for both roles. It is released in lockstep with the rest of the
`uds_stack` workspace (`uds_protocol`, `uds_session`, `uds_services` and
`simple_doip`), all at one shared version.

### Added

- `DoIpTransport<E, CONNECTIONS>`, the server-side `uds_services::UdsTransport`
  over any `simple_doip::service::DiagnosticEntity`. It carries requests and
  responses as `DoIP` diagnostic messages, sends each response on the connection
  of the tester it is addressed to, and uses the entity's clock as the stack's
  time. It closes the connection after every positive `ECUReset` response
  (REQ 7.11) and after a positive `DiagnosticSessionControl` response to a session
  change that leaves the running software (REQ 7.9). `CONNECTIONS` must be at
  least the entity's own connection count, which is checked at compile time.
- `DoIpClientTransport<C, QUEUE, PEERS>`, the client-side
  `uds_services::UdsTransport` and `ClientTransport` over any
  `simple_doip::service::TesterConnection`. It queues one request behind another,
  turns a refused request into a failed confirmation, and reconnects and
  re-activates routing after a server closes the connection (REQ 7.8, REQ 7.10).
  It reports each close as expected or unexpected, and sends a request on a new
  connection when a late reply to an earlier one could otherwise be mistaken for
  its answer. Received periodic responses are reported as
  `TransportEvent::Periodic`. `with_max_data_size` passes in the entity's
  advertised *Max. data size*, so `outbound_max` reports the true limit.
- `profile::bench_reloads()`, the conventional `tP6` reload pair (2000 ms and
  5000 ms) for a bench setup. It is a function and not a `Default`, because these
  values are not a vehicle configuration.
- `mapping`, the address conversions between `uds_session` and `simple_doip`
  (`to_logical`, `from_logical`, `target_of`), and `MappingError` for the remote
  message types, which `DoIP` cannot carry.
- `Error` for the server transport and `ClientTransportError` for the client
  transport. A conformant connection close is reported as an event, not as an
  error.
- `no_std` and allocation-free by default (`default = []`), with no async runtime
  named. `alloc` and `std` features forward to `simple_doip` and `uds_protocol`, and
  `std` adds `std::error::Error`. `unsafe` is forbidden. MSRV 1.91.

### Known limitations

- A server cannot send periodic responses (payload type `0x8004`, REQ 7.16), so
  `ReadDataByPeriodicIdentifier` cannot be served. A server ignores a received one.
- A suppressed session change (`10 82`) or reset (`11 81`) sends no response, so
  the prescribed connection close is not made.
- `uds_services` has no hook to run a reset on its response's confirmation, so an
  `ECUReset` handler that resets on its own does so before the connection closes.
- A client's functional request reaches only the one entity its connection is to.
  Fanning a request out to several entities is not supported.
