# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0-alpha.1](https://github.com/luminartech/uds_stack/releases/tag/uds_on_ip/v0.2.0-alpha.1) - 2026-09-30

### Added

- make the stack a cargo workspace
- key both clause 8 connection requirements on the octet already read
- [**breaking**] implement uds_services::UdsTransport and delete the event mirror
- *(transport)* distinguish a truncated message from a whole one
- *(transport)* add the crate's outward interface
- *(mapping)* own the address conversions and the T_AE rejection
- *(client)* add the async client skeleton
- *(handler)* define the request-handler seam
- *(profile)* add the UDSonIP application profile
- *(mapping)* map T_PDU service primitives onto DoIP_PDU
- *(error)* add the error taxonomy with a core::error canary
- *(session)* define the session-layer seam
- *(addressing)* add ISO 14229-2 addressing primitives
- *(iris-diagnostics-client)* add crate with connect, config and test ECU

### Fixed

- satisfy the checks the shared workflow runs that the local one did not
- [**breaking**] read the payload type from the header so 0x8004 stays decodable
- *(transport)* [**breaking**] let the event outlive the borrow that produced it
- apply whole-branch review fix wave
- *(transport)* make the socket field's suppression self-enforcing
- *(profile)* drop a tautological test and tighten the reload accessor
- commit the regenerated lockfile and catch section-header deps
- scope the upward-seam scan to declarations, not prose
- declare the modules, and derive the primitives from the standard
- *(dft)* retry the initial sensor connect
- *(uds-on-ip)* never send TesterPresent while awaiting a response
- *(dft-lib)* abort a flash when the transport resets mid-sequence
- *(uds_on_ip)* describe routing-activation denials per ISO 13400 code
- *(diagnostics)* surface routing-activation denial as a non-retryable error (main.rs:83 review)
- *(envision)* tune diagnostics-dump DID set + capture raw bytes on decode failure
- *(uds_on_ip)* suppress in-loop TP once ECU enters NRC 0x78 pending cycle
- *(uds_on_ip)* suppress in-loop TP sends while ECU is actively pending
- *(uds_on_ip)* restore reconnected_without_resend + timeout-path re-send
- *(uds_on_ip)* return ReconnectedWithoutResponse on initial-send reconnect
- *(uds_on_ip)* update last_activity after reconnect re-send; fix response_sid doc

### Other

- one lint standard for the workspace, and the gaps written down
- make the shared licence and governance files hold, and enforce it
- correct the crate scopes the READMEs still state from before the split
- repoint what the reorganisation left pointing at the old repositories
- rewrite the crate READMEs as one set
- one licence text, symlinked into each crate
- *(uds_on_ip)* cut ARCHITECTURE.md down to what checks out today
- give every crate a readme field, and drop a stale exclude
- one pipeline over the workspace
- rewrap doc comments to 92 columns
- restore the line-width hook to rust sources, and clear the hook backlog
- reformat the whole workspace to 92 columns
- *(uds_on_ip)* drop the section 9 reference from distribution constraints
- *(uds_on_ip)* take internal distribution topology out of the crate
- *(arch)* describe the stack that exists, not the one that was replaced
- pin the two neighbour facts this crate's design rests on
- [**breaking**] fold post_exchange into TransportEvent::Closed's expected flag
- [**breaking**] drop inbound_max, which has no producer, consumer or destination
- [**breaking**] delete Timing and Spacing, taking uds_session::Reloads whole
- ignore .DS_Store and drop the execution scaffolding
- [**breaking**] retire the defaults, signatures and citations a caller cannot trust
- [**breaking**] take uds_session's Reloads instead of keeping our own
- *(mapping)* read the ack code, not the payload type
- put the seam holes in the build instead of in a paragraph
- narrow the surface to what this crate can actually deliver
- key the profile guard on the invariant, not the phrasing
- hedge the dependency edge that does not exist yet
- describe the crate as the transport it now is
- *(profile)* name the timing parameters after the standard
- [**breaking**] drop the runtime and embedded-io from the manifest
- [**breaking**] delete the session and handler seams
- [**breaking**] take the ISO 14229-2 vocabulary from uds_session
- [**breaking**] delete the interim client
- add the API-shape implementation plan and ignore SDD scratch
- refresh the lockfile for simple_doip 0.6.0
- correct 8.4 — the protocol crates are public, and Kellnr proxies
- split publication into internal and public tracks, and resolve the
- verify the SDK bundling constraint, and record the two-copy problem
- correct the architecture against the code, and resolve three
- [**breaking**] retire the current implementation to legacy/
- describe the diagnostics stack architecture
- link sibling crates by URL rather than workspace path
- add the dual licence texts
- make uds_on_ip build standalone outside the dft workspace
- *(uds-on-ip)* take simple_doip 0.5 and surface the DoIP message timeout
- *(uds_on_ip)* migrate transport seam to uds_protocol 0.1 (owned bytes at boundary)
- bump simple_doip and adapt to its borrowed-message API
- scrub stale VCC/DoIPInt references
- *(release)* bump DFT stack to 0.5.0
- apply en-US spelling fixes via typos -w
- rebrand pre-release cleanup — icons, authors, target label
- Update changelog for tester present
- reject 2-byte NACKs as malformed + regression test
- apply rustfmt to uds_on_ip TP-NACK fix
- ignore stray cross-SID responses and re-send immediately after reconnect
- loosen timings in tester_present_fires_during_nrc78_wait
- gate post-reconnect TP on keepalive-active predicate
- switch to response_pending_timeout (P2*) after NRC 0x78
- adversarial-review fixes for the in-request TP keep-alive
- address Copilot review on #543
- Add tests to verify we send TP messages 2s after our last message sent
- Initial fix tester present commit
- format
- Ensure that we still send tester present when handling reconnections
- unify versions across internal an submodule crates
- cargo fmt
- re-enable signature check
- Address structural issues with refactor
- major dft_refactor Phase 1
- Address issues with reconnection
- handle suppressed responses
- Add tester present functionality to uds_on_ip client
- Standardize on rust 2024
- Remove tester present logic from simple_doip
- Address connectivity issues with udson_ip
- Add reconnect logic to uds_on_ip
- Massive refactor of archive format
- Hook uds_on_ip up with simple_doip
- Validate request/response pairs
- Add propper support for 0x78 NRC
- Add support for tester present
- Create uds_on_ip crate to allow uds session management over DoIP
