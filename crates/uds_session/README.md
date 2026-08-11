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

The requirement set is published as Sphinx + sphinx-needs documentation.

<!-- TODO: link the GitHub Pages URL once the docs workflow is in place. -->

Each requirement carries:

- `origin` — whether it was transcribed from a standard or derived from a design decision.
- `source` — where in that standard it comes from.
- `integrity_level` and `target_level` — what is currently substantiated by evidence, and
  what it is expected to reach. The gap between them is the outstanding work, and is
  reportable rather than tacit.
- `status` — requirement IDs are allocated, never renumbered and never reused. Once a
  requirement is approved and linked externally, its ID is fixed for the life of the crate.

The `needs` builder produces `needs.json`, consumed by a separate qualification repository
that holds the safety argument.

## Usage

<!-- TODO: write once there is an API. -->

## Building

```console
$ cargo test
$ uv sync
$ uv run sphinx-build -b html -W docs docs/_build/html
$ uv run sphinx-build -b needs -W docs docs/_build/needs
```

## Contributing

<!-- TODO: link CONTRIBUTING.md once written. -->

## Licence

<!-- TODO: decide. -->
