# Security policy

Covers every crate in this repository: `uds_protocol`, `uds_session`,
`uds_services`, `simple_doip` and `uds_on_ip`.

## Reporting a vulnerability

Report security issues through GitHub's private vulnerability reporting: open
the [Security tab](https://github.com/luminartech/uds_stack/security) and
choose **Report a vulnerability**. That opens a private advisory visible only
to the maintainers.

Please do not open a public issue for a security report.

A report is most useful with the crate and version, the feature set enabled,
and a byte sequence or test case that reproduces the behaviour.

## Supported versions

Every crate here is pre-1.0. Fixes land on the latest published version; there
are no maintained release branches. Versions are independent, so name the crate
as well as the version.

## Scope

These crates implement diagnostic protocols that were not designed with
transport security. Several properties that look like vulnerabilities are the
specification's design rather than defects here, and knowing which is which is
most of assessing a report.

**The protocols carry no transport security.** DoIP connections on `TCP_PORT`
(`13400`) are in the clear. `TCP_TLS_PORT` (`3496`) is defined by
ISO 13400-2, and nothing in this repository uses it.

**Routing activation is not authentication.** It is an addressing handshake.

**SecurityAccess is not a cryptographic primitive.** ISO 14229-1's seed-and-key
exchange is specified without naming an algorithm, and the strength of any
particular scheme belongs to whoever implements it. `uds_protocol` encodes and
decodes the messages; it takes no position on the algorithm behind them.

**Access control over a diagnostic session belongs above these layers.** A
server that answers a request has already decided it is willing to; nothing in
these crates makes that decision.

In scope is anything that makes a crate misbehave on attacker-supplied bytes: a
panic, an out-of-bounds read, an unbounded allocation, a decode that accepts a
frame it should reject, a state machine that can be driven into an
unrecoverable state, or a hang reachable from the wire.

Panics are worth reporting even where they look benign. Every crate here denies
the panicking constructs — `unwrap`, `expect`, `panic`, `unreachable`, `todo`,
`unimplemented` — in production code, from one `[workspace.lints]` table, and
forbids `unsafe` outright. A panic in a diagnostic stack is an unhandled failure
in a safety-related component, so one reachable from the wire is a defect
regardless of how it is triggered.

Where a panic survives, it is a written-down exception rather than an oversight:
each carries a `#[expect(…, reason = …)]` naming what makes it unreachable, and
the few that are reachable are declared in their function's `# Panics` section.
A report that shows one of those reasons to be wrong is exactly the kind that is
most useful.

Two crates, `uds_protocol` and `simple_doip`, still allow slice indexing and
unchecked arithmetic in production code; both predate the standard, and
`CONTRIBUTING.md` records the counts. A panicking index reachable from
attacker-supplied bytes is in scope there as anywhere.
