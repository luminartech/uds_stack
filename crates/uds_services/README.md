# uds_services

ISO 14229-1 clause 8.7 server response implementation rules: typed UDS service
dispatch over caller-defined identifiers.

## Status

**Pre-implementation.** No behaviour is implemented yet.

The architecture is authored as a sphinx-needs set under
[`docs/architecture/`](docs/architecture/) and is meant to be read rendered —
`uv sync --frozen && just html`. It carries twenty-four architecture elements
(`UDSSVC_ARCH_####`), each recording either the clause that forces it or the
reasoning behind choosing it, and an
[open questions](docs/architecture/open-questions.rst) page for what is still
unsettled.

[`docs/design.md`](docs/design.md) is the earlier design conversation, kept for
the reasoning it records. Where the two disagree, the architecture set is
current.

## What this is

The point where an application meets a diagnostic stack, in both directions: the interface
by which a server integrates the stack, and the set of requests available to a client
application. Everything below it deals in bytes — this is the last layer that understands
UDS.

Its scope is ISO 14229-1's *behaviour*. That document is split in two across the stack:
[`uds_protocol`](https://github.com/luminartech/uds_protocol) owns the format — the bits,
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
declines: `uds_on_ip`'s client returns raw bytes and notes that a negative
response "is a response, and interpreting it belongs to a higher layer."

One identifier catalogue, both roles. A vehicle programme that builds an ECU and
a tester from the same definitions gets a compiler error when only one of them
was updated, rather than a mismatch found on a bench.

## What this is not

Not a codec — messages are [`uds_protocol`](https://github.com/luminartech/uds_protocol).
Not a session layer — session timing is `uds_session`, though deciding to send
a response-pending is this crate's (ISO 14229-2 makes it turn on whether the
server supports the service). Not a
connection manager: the diagnostic conversation is portable across transports,
connection setup is not, and no API here pretends otherwise. Not
transport-aware: a handler that knows how to fetch a data identifier has nothing
to say about IP, so the transport binding is an optional feature and the same
server works over DoIP or CAN.

## Layering

```
                    application
                         │  typed service handlers
                  uds_services          ISO 14229-1 cl. 8.7   ← this crate
                         │  byte seam, owned by the binding
          ┌──────────────┴──────────────┐
      uds_on_ip                   uds_on_can
      (ISO 14229-5)               (ISO 14229-3)
          │ wraps                       │ wraps
          └────────► uds_session ◄──────┘        ISO 14229-2
          │                             │
      simple_doip                 ISO 15765-2
      (ISO 13400-2)
```

One crate per ISO document, so "does this belong here?" is answered by asking
which document specifies the behaviour.

## Design constraints

`no_std` and allocation-free. Responses are written into a caller-supplied sink
rather than returned as a `Vec`, and no public type carries a `Vec` or a
`String`. This is designed in rather than deferred: alloc-freedom cannot be
retrofitted, because the signatures that make an API alloc-free are the ones
callers depend on.

## Relationship to the standards

Neither licence granted here conveys any right in the ISO standards this crate
implements or cites, which remain ISO's. No standard's text is reproduced.

## Licence

MIT OR Apache-2.0, at your option.
