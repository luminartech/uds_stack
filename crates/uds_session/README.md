# uds_session

`uds_session` owns ISO 14229-2:2021, the session layer: a transport-agnostic,
`no_std`, allocation-free sans-io state machine implementing both the client
and the server role. In its documentation, a table, figure, clause or
requirement cited with no document named is ISO 14229-2:2021's; any other
document is named where it is cited.

## Where this fits

`uds_session` sits in the middle of the [`uds_stack`](https://github.com/luminartech/uds_stack)
workspace. It depends on nothing else in the stack — no other crate's message
format or transport is any of its business — and is driven directly by
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)
(ISO 14229-1's behaviour), which owns the `Client`/`Server` instance and supplies
every input.

## Status

Pre-implementation. The public surface is complete — every type and method a
caller will use is already named and documented — but no behaviour is
implemented yet: every entry point is `todo!()`, carrying the requirement it
will satisfy. This crate is not yet published; see the
[workspace README](https://github.com/luminartech/uds_stack#status) for where
it and its siblings stand.

Requirements are authored before the code that satisfies them. The ordering is
evidence that cannot be reconstructed afterwards, so implementation follows the
published requirement set rather than preceding it.

## What this is

The session layer sits between a diagnostic application and a transport layer. It owns the
timing that keeps a diagnostic session alive and bounds how long each side waits, and it
maps between the application's service primitives and the transport's.

- **Sans-io.** No clock is read, no transport is called, no executor is involved. The
  caller supplies a monotonic timestamp together with inbound transport events, and drains
  the resulting actions.
- **`no_std`, no allocation.** Storage is provided by the caller, so sizing is a deployment
  decision rather than a compile-time constant of this crate.
- **Transport-agnostic.** The standard defines this layer to be independent of the
  transport beneath it, and the crate follows: the same state machine serves DoCAN, DoIP,
  FlexRay, DoK-Line and CXPI.
- **Both roles.** Client and server are separate state machines over shared vocabulary.
- **Panic-free and `unsafe`-free by construction.** Enforced by the lint configuration in
  `Cargo.toml`, which applies to every target rather than to the crate root alone.

## What this crate does not do

- **Diagnostic services themselves (ISO 14229-1).** No request or response payload is
  interpreted. The crate does not know what a service does; it knows when a message may be
  sent and how long a peer may take to answer. That is
  [`uds_protocol`](https://crates.io/crates/uds_protocol)'s and
  [`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s
  job, not this crate's.
- **Transport and network layers.** Segmentation, flow control and addressing on the wire
  belong beneath this crate — see
  [`simple_doip`](https://crates.io/crates/simple_doip) and
  [`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip).
- **Performance obligations the crate cannot discharge.** Network transmission delays are
  properties of the vehicle network, and the time an application takes to produce a final
  response is a property of that application. Both bound the timing values a deployment
  chooses; neither is behaviour a state machine can implement. They are configuration
  inputs here, not requirements on this crate.

## Requirements

The requirement set is published as Sphinx + sphinx-needs documentation at
<https://luminartech.github.io/uds_stack/>, rebuilt from `main` on every push, alongside
the rest of the workspace's requirement and architecture set.

The published site is HTML only. `needs.json` is deliberately not served from it: that
export is consumed as evidence against a committed snapshot, which a URL rebuilt on every
push cannot be.

Each requirement carries:

- `origin` — whether it was transcribed from a standard or derived from a design decision.
- `source` — where in that standard it comes from: clause, table or figure. A derived
  requirement carries no `source`; it states its reasoning in a `Rationale:` paragraph
  instead.
- `integrity_level` and `target_level` — what is currently substantiated by evidence, and
  what it is expected to reach. The gap between them is the outstanding work, and is
  reportable rather than tacit.
- `status` — lifecycle stage of the requirement: `draft` (authored, not yet reviewed; IDs may
  still move), `review` (under review), `approved` (reviewed and accepted; ID is permanent),
  or `obsolete` (withdrawn; ID retained and never reused).

While the set is draft its IDs may still move: it is numbered in document order, so a
restructuring renumbers it. Once a requirement is approved and linked externally, its ID
is fixed for the life of the crate, and the ID of an obsolete requirement is never reused.

## Relationship to the standards

This crate is not a copy of any standard, and reading it is no substitute for holding one.
The requirement set is an independent restatement: every requirement is written in this
project's own words, and `source` records the clause, table or figure a requirement came
from rather than reproducing what it says. No text, table or figure from any ISO standard
is reproduced here.

The standards' own vocabulary is kept deliberately — primitive and parameter names such as
`T_Data.ind` and `tS3_Server` appear unchanged, because a requirement that renamed them
would be untraceable and an implementation that renamed them would be harder to review
against the standard.

Neither licence below grants any right in ISO 14229-1, -2 or -5, which remain ISO's — see
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate in this stack.

## Usage

*API documentation to be written as the implementation takes shape.*

## Building

```console
$ cargo test
```

The requirement set is built from the workspace root, alongside every other crate's — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md)
for the toolchain, `just html` (browsable output), `just summary` (counts, source coverage,
ID gaps), and the full gate, `just check-all`.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.

The same licence covers the requirement set under
[`docs/`](https://github.com/luminartech/uds_stack/tree/main/docs), including
each requirement's trace to the clause it came from.
