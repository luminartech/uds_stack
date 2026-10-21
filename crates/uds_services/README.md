# uds_services

Everything in ISO 14229-1:2020 that is not the message format: typed UDS service
dispatch over caller-defined identifiers, and the clause 8.7 server response
implementation rules. In its documentation, a table, figure, clause or
requirement cited with no document named is ISO 14229-1:2020's; any other
document is named where it is cited.

## Where this fits

`uds_services` is the last layer in the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace that
understands UDS: everything below it deals in bytes. It depends on
[`uds_protocol`](https://crates.io/crates/uds_protocol) (ISO 14229-1 messages)
and [`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)
(ISO 14229-2 session timing), and drives the latter by value. Everything above
it is application code; everything it calls through is a transport binding,
such as
[`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
(ISO 14229-5), that implements the `UdsTransport` trait this crate declares.
This crate never depends on a binding: the Cargo edge runs the other way.

## Status

**Alpha.** The server and client roles run end to end over
any `UdsTransport`; `DataTransfer` stages the download, and its upload and file
transfer are not supported (0x11). It releases in lockstep with the rest
of the stack, at one shared version. See
the
[workspace README](https://github.com/luminartech/uds_stack#status) for how
that compares to the rest of the stack.

The architecture is a sphinx-needs set under
[`docs/architecture/uds_services/`](https://github.com/luminartech/uds_stack/tree/main/docs/architecture/uds_services)
at the workspace root, rendered at
<https://luminartech.github.io/uds_stack/architecture/uds_services/index.html>.
Each architecture element (`UDSSVC_ARCH_####`) records the clause that forces
it or the reasoning behind choosing it, and an
[open questions](https://github.com/luminartech/uds_stack/blob/main/docs/architecture/uds_services/open-questions.rst)
page holds what is still unsettled.

## What this is

The point where an application meets a diagnostic stack, in both directions: the interface
by which a server integrates the stack, and the set of requests available to a client
application. Everything below it deals in bytes; this is the last layer that understands
UDS.

Its scope is ISO 14229-1's *behavior*. That document is split in two across the stack:
[`uds_protocol`](https://crates.io/crates/uds_protocol) owns the format, the bits,
the bytes, and which messages are valid, and this crate owns everything else in it,
clause 8.7's server response implementation rules above all.

### The server side

ISO 14229-1 clause 8.7 specifies the whole validation sequence: which checks
run in which order, which negative response code each failure produces, and
when the correct answer is no response at all. This crate runs it, so an
application defines its identifiers, implements the services it supports, and
everything else follows:

- A service that is not implemented answers `serviceNotSupported`.
- An identifier outside the supported set answers `requestOutOfRange`.
- Session and security preconditions produce the right in-session codes.
- A functionally addressed request that the server does not support gets
  **silence**, not a negative response, because the same request reached every
  server on the bus.

Identifiers are the application's own, so adding one breaks the application's
exhaustive `match` rather than failing quietly at run time.

### The client side

A client application names a service and its parameters in the same identifier
vocabulary its server handlers use, and gets back a typed result. It never
assembles request bytes or parses responses.

That includes interpreting negative responses: a transport binding maps bytes
onto its protocol and has no view of a negative response at all.

One identifier catalog, both roles. A vehicle program that builds an ECU and
a tester from the same definitions gets a compiler error when only one of them
was updated, rather than a mismatch found on a bench.

## Scope

Messages are [`uds_protocol`](https://crates.io/crates/uds_protocol)'s to encode
and decode, and session timing is
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)'s.
Deciding to send a response-pending is this crate's, because ISO 14229-2 makes it
turn on whether the server supports the service. Connection setup is the
binding's: the diagnostic conversation is portable across transports, and
connection setup is not. The application names the binding, so a handler that
fetches a data identifier is written once for every transport.

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
which document specifies the behavior. ISO 14229-1 is the single exception:
`uds_protocol` takes its format and this crate takes the rest. `uds_on_can` is
drawn to show the shape the design admits, not a crate that exists in this
workspace.

## Design constraints

`no_std` and allocation-free. Responses are written into a sink this crate
owns rather than returned as a `Vec`, and no public type carries a `Vec` or a
`String`.

There is no feature table: this crate has no `[features]` in `Cargo.toml`. It
is `no_std` and allocation-free in every configuration, with no cfg anywhere in
`src/`.

## Relationship to the standards

Neither license granted here conveys any right in the ISO standards this crate
implements or cites, which remain ISO's. No standard's text is reproduced. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate in
this stack.

## Contributing

Pull requests, bug reports and questions are welcome; see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.
