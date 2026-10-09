# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) (treating
`0.x` breaking changes as minor bumps, per the Cargo/SemVer convention for
pre-1.0 crates).

## [Unreleased]

First release. `uds_session` implements ISO 14229-2:2021, the UDS session layer, as a
transport-agnostic sans-io state machine for both the client and the server role. It
is released in lockstep with the rest of the `uds_stack` workspace (`uds_protocol`,
`uds_services`, `simple_doip` and `uds_on_ip`), all at one shared version.

### Added

- `Server<A>`, the session layer in the server role. It owns `A` caller-sized
  `Association` slots and runs the `tS3_Server`, `tP2_Server` and `tP2*_Server`
  timers from `ServerParams`. `A` must be at least 1, which is checked at compile
  time. Its outputs are `ServerOutput`: `Transmit` (`T_Data.req`), `Indicate`
  (`S_Data.ind`), `Confirm` (`S_Data.conf`), `SessionTimeout` when `tS3_Server`
  ends a non-default session, and `ResponseOverrun` when `tP2_Server` is about to
  expire with no response sent. `ServerParams::response_pending_lead` sets how early
  that overrun is reported, so a response-pending message can still go out inside
  the window. `is_well_formed` checks the lead against both windows.
- `Client<K, PHYS, FUNC, R>`, the session layer in the client role, with separate
  arrays of physical and functional channel slots (`PhysicalSlot`,
  `FunctionalSlot<R>`, where `R` is the responders tracked per functional channel).
  The `tP_Client` response window is loaded from a channel's `Reloads` (default and
  enhanced), and `tP3` request spacing comes from `ChannelParams`. Outputs are
  `ClientOutput`: `Transmit`, `Indicate`, `Confirm`, `ResponseTimeout`,
  `KeepAliveDue`, and `Capacity` for a functional responder the table has no room
  for. Channels are opened, withdrawn, reset and reconfigured through
  typed handles (`PhysicalChannelId`, `FunctionalChannelId`, `ChannelId`). An
  optional `Tag` type parameter brands the handles, so a handle from one client
  does not compile when passed to another.
- Keep-alive modes `FunctionalKeepAlive` (one client-wide `tS3_Client`) and
  `PhysicalKeepAlive` (one per physical channel), chosen at creation through the
  sealed `KeepAliveMode` trait. Methods that only make sense in one mode exist only
  on that mode.
- Inputs as the ISO 14229-2 service primitives: `s_data_req`, `t_data_som_ind`,
  `t_data_ind`, `t_data_conf` and `tick`, each taking a caller-supplied `Timestamp`,
  plus `Server::completion_report` for a request that was handled with no response.
  `next_deadline` tells the caller when to call `tick` next.
- `Reaction`, which each input returns, is drained for its outputs. `Reaction::finish`
  gives a `Finished` with the input's outcome and any outputs not yet drained, so
  none are lost. A refused input reports a `Rejection` that lists every `Cause` and
  the content each one requires.
- The ISO 14229-2 vocabulary: addressing (`Ai`, `Address`, `AddressExtension`,
  `Mtype`, `TaType`, `ChannelAddressing`, `PeerIdentity`), message classification
  (`ClientTx`, `ClientRx`, `ServerTx`, `ServerRx`, `ExpectedResponses`,
  `SessionSelection`, `Solicitation`) and results (`SResult`, `TransportError`).
- `Timestamp`: a caller-supplied 32-bit millisecond count that wraps.
  `interval_since`, `has_reached` and `until` handle the wrap; the derived `Ord`
  does not.
- `no_std` with no allocation and no dependencies. Storage is supplied by the caller
  through const generics. No clock is read, no transport is called and no executor
  is needed. `unsafe` is forbidden.
- MSRV 1.91.
