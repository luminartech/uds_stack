# uds_services

ISO 14229-1 clause 8.7 server response implementation rules: typed UDS service
dispatch over caller-defined identifiers.

## Status

**Pre-implementation.** No behaviour is implemented yet. The starting design is
in [`docs/design.md`](docs/design.md), including the open questions that need
settling before the shape is fixed.

## What this is

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

Identifiers are the application's, supplied as associated types, so adding one
breaks the application's own exhaustive `match` rather than failing quietly at
run time.

## What this is not

Not a codec — messages are [`uds_protocol`](https://github.com/luminartech/uds_protocol).
Not a session layer — timers and response-pending are `uds_session`. Not
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
