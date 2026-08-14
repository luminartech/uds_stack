# uds_session

ISO 14229-2 session layer services for UDS diagnostics: a transport-agnostic, `no_std`,
allocation-free sans-io state machine implementing both the client and the server role.

## Status

Pre-implementation. No behaviour is implemented yet.

Requirements are authored before the code that satisfies them. The ordering is evidence
that cannot be reconstructed afterwards, so implementation follows the published
requirement set rather than preceding it.

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

## Scope

In scope:

- Session timing for the server: the session timer that holds a non-default session open,
  and the response timing that bounds when a response must begin, including enhanced
  response timing.
- Session timing for the client: the timer awaiting each response, the keep-alive timer,
  and the minimum spacing between consecutive requests.
- Session layer error handling on both sides.
- The service interface: the request, indication and confirmation primitives exchanged with
  the application, and their mapping onto transport primitives.

Out of scope:

- **Diagnostic services themselves (ISO 14229-1).** No request or response payload is
  interpreted. The crate does not know what a service does; it knows when a message may be
  sent and how long a peer may take to answer.
- **Transport and network layers.** Segmentation, flow control and addressing on the wire
  belong beneath this crate.
- **Performance obligations the crate cannot discharge.** Network transmission delays are
  properties of the vehicle network, and the time an application takes to produce a final
  response is a property of that application. Both bound the timing values a deployment
  chooses; neither is behaviour a state machine can implement. They are configuration
  inputs here, not requirements on this crate.

## Requirements

The requirement set is published as Sphinx + sphinx-needs documentation at
<https://luminartech.github.io/uds_session/>, rebuilt from `main` on every push.

The published site is HTML only. `needs.json` is deliberately not served from it: the
private qualification repository consumes that file as evidence and pins to a committed
snapshot, which a URL rebuilt on every push cannot be.

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

Requirement IDs are allocated once and never renumbered or reused. Once a requirement is
approved and linked externally, its ID is fixed for the life of the crate.

The `needs` builder produces `needs.json`, consumed by a separate qualification repository
that holds the safety argument.

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

ISO holds copyright in the standards cited. Implementing or verifying against this crate
requires your own licensed copies of ISO 14229-1, ISO 14229-2 and ISO 14229-5: the
standards are cited by designation and clause so that a reader who holds them can check
the trace, while the requirements themselves are written to be verifiable without them.

## Usage

*API documentation to be written as the implementation takes shape.*

## Building

Recipes live in the `justfile`; `just --list` shows them all. `--frozen` refuses to
re-resolve, so the toolchain that produced a given `needs.json` stays recoverable.

```console
$ just check       # every pre-commit hook over every file — the gate before pushing
$ just html        # build the requirement set for reading locally
$ just summary     # counts, source coverage, ID gaps
$ cargo test
```

Without `just`:

```console
$ cargo test
$ uv sync --frozen
$ uv run --frozen sphinx-build -b html -W docs docs/_build/html
$ uv run --frozen sphinx-build -b needs -W docs docs/_build/needs
```

## Contributing

*Contribution guidelines are in development.*

Unless you state otherwise, any contribution you intentionally submit for inclusion in this
work, as defined in the Apache-2.0 licence, is dual licensed as below, with no additional
terms or conditions.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT License ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

One licence covers the crate's source and the requirement set under `docs/` alike,
including each requirement's trace to the clause it came from. The whole of what this crate
claims to implement, and where each claim comes from, is public and checkable.

What is held separately and offered commercially is the qualification package: assumptions
of use, failure analysis, integration guidance, and platform test evidence. Neither licence
grants any right in that material. A safety programme cannot take an unqualified component
as-is whatever its licence, so it is that package, rather than the licence here, that makes
this crate usable in a certified item. Contact MicroVision if you need it.

Neither licence grants any right in the ISO standards the requirements cite, either — those
remain ISO's. See [Relationship to the standards](#relationship-to-the-standards).
