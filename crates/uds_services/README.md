# uds_services

Everything in ISO 14229-1 that is not the message format: typed UDS service
dispatch over caller-defined identifiers, and the clause 8.7 server response
implementation rules.

## Where this fits

`uds_services` is the last layer in the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace that
understands UDS: everything below it deals in bytes. It depends on
[`uds_protocol`](https://crates.io/crates/uds_protocol) (ISO 14229-1 messages)
and [`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)
(ISO 14229-2 session timing), and drives the latter by value. Everything above
it is application code; everything it calls through is a transport binding —
today that means
[`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
(ISO 14229-5) — that implements the `UdsTransport` trait this crate declares.
This crate never depends on a binding: the Cargo edge runs the other way.

## Status

**Pre-implementation, version 0.0.0, not published** (`publish = false` in
`Cargo.toml`). See the
[workspace README](https://github.com/luminartech/uds_stack#status) for how
that compares to the rest of the stack.

The architecture is authored as a sphinx-needs set under
[`docs/architecture/`](https://github.com/luminartech/uds_stack/tree/main/docs/architecture)
at the workspace root — not inside this crate's own directory — and is meant to
be read rendered, at <https://luminartech.github.io/uds_stack/>. It carries
forty-two architecture elements (`UDSSVC_ARCH_####`), each recording either
the clause that forces it or the reasoning behind choosing it, and an
[open questions](https://github.com/luminartech/uds_stack/blob/main/docs/architecture/open-questions.rst)
page for what is still unsettled. That set is provisional in the same sense as
every crate's: it supersedes any `ARCHITECTURE.md`, and nothing here carries
one.

## What this is

The point where an application meets a diagnostic stack, in both directions: the interface
by which a server integrates the stack, and the set of requests available to a client
application. Everything below it deals in bytes — this is the last layer that understands
UDS.

Its scope is ISO 14229-1's *behaviour*. That document is split in two across the stack:
[`uds_protocol`](https://crates.io/crates/uds_protocol) owns the format — the bits,
the bytes, and which messages are valid — and this crate owns everything else in it.
Clause 8.7, the server response implementation rules, is the densest part of that scope
and the reason the crate exists, but it is not the boundary.

### The server side

A UDS server has to do more than answer the requests it supports. ISO 14229-1
clause 8.7 specifies the whole validation sequence — which checks run in which
order, which negative response code each failure produces, and when the correct
answer is no response at all. Those rules are easy to get subtly wrong, and the
mistakes are invisible when testing against a cooperative client.

This crate centralises them, so an application defines its identifiers,
implements the services it supports, and everything else follows:

- A service that is not implemented answers `serviceNotSupported`.
- An identifier outside the supported set answers `requestOutOfRange`.
- Session and security preconditions produce the right in-session codes.
- A functionally addressed request that the server does not support gets
  **silence**, not a negative response — because the same request reached every
  server on the bus.

Identifiers are the application's own, so adding one breaks the application's
exhaustive `match` rather than failing quietly at run time.

### The client side

A client application names a service and its parameters in the same identifier
vocabulary its server handlers use, and gets back a typed result. It never
assembles request bytes or parses responses.

That includes interpreting negative responses, which the layer below explicitly
declines: `uds_on_ip`'s transport has no view of a negative response at all — it
maps bytes onto `DoIP`, and interpreting what came back belongs to this layer.

One identifier catalogue, both roles. A vehicle programme that builds an ECU and
a tester from the same definitions gets a compiler error when only one of them
was updated, rather than a mismatch found on a bench.

## What this is not

Not a codec — messages are [`uds_protocol`](https://crates.io/crates/uds_protocol).
Not a session layer — session timing is
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session),
though deciding to send a response-pending is this crate's (ISO 14229-2 makes it turn on
whether the server supports the service). Not a
connection manager: the diagnostic conversation is portable across transports,
connection setup is not, and no API here pretends otherwise. Not
transport-aware: a handler that knows how to fetch a data identifier has nothing
to say about IP, so the transport binding is an optional feature and the same
server works over DoIP or CAN.

## Layering

```
                    application
                         │  typed service handlers
                  uds_services          ISO 14229-1 behaviour ← this crate
                         │  byte seam, owned by the binding
          ┌──────────────┴──────────────┐
      uds_on_ip                   uds_on_can
      (ISO 14229-5)               (ISO 14229-3, not yet built)
          │ wraps                       │ wraps
          └────────► uds_session ◄──────┘        ISO 14229-2
          │                             │
      simple_doip                 ISO 15765-2
      (ISO 13400-2)
```

One crate per ISO document, so "does this belong here?" is answered by asking
which document specifies the behaviour. ISO 14229-1 is the single exception —
too large for that rule to settle, so `uds_protocol` takes its format and this
crate takes the rest. `uds_on_can` is drawn to show the shape the design buys,
not a crate that exists in this workspace yet.

## Design constraints

`no_std` and allocation-free. Responses are written into a caller-supplied sink
rather than returned as a `Vec`, and no public type carries a `Vec` or a
`String`. This is designed in rather than deferred: alloc-freedom cannot be
retrofitted, because the signatures that make an API alloc-free are the ones
callers depend on.

There is no feature table: this crate has no `[features]` in `Cargo.toml`. It
is `no_std` and allocation-free in every configuration, with no cfg anywhere in
`src/`.

## Relationship to the standards

Neither licence granted here conveys any right in the ISO standards this crate
implements or cites, which remain ISO's. No standard's text is reproduced. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate in
this stack.

## Contributing

Pull requests, bug reports and questions are welcome — see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option. This crate is not published to crates.io
(`publish = false`) while it remains pre-implementation.
