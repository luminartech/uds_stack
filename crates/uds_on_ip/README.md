# uds_on_ip

`uds_on_ip` owns **ISO 14229-5:2022 (`UDSonIP`)**: the application profile that
binds Unified Diagnostic Services to a `DoIP` transport. It owns two layers of
that standard: clause 8, the application profile (the `A_PDU` format, TCP
connection handling around `DiagnosticSessionControl` and `ECUReset`, and which
timing parameters apply), and clause 11, the mapping of `T_PDU` service
primitives onto `DoIP`'s. In its documentation, a table, figure, clause or
requirement cited with no document named is ISO 14229-5:2022's; any other
document is named where it is cited.

## Where this fits

```text
  consuming application
       ↕  typed service traits / typed client calls
  uds_services      the driver: owns Client/Server, declares UdsTransport
       ↓  UdsTransport
  uds_on_ip         ISO 14229-5 profile + DoIP mapping  ← this crate
       ↓
  simple_doip       ISO 13400-2
```

This crate is **wholly below** the session layer in the
[`uds_stack`](https://github.com/luminartech/uds_stack) workspace. It hosts no
driver, calls nothing upward, and knows nothing about services. It implements
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s
`UdsTransport` trait, and depends on
[`simple_doip`](https://crates.io/crates/simple_doip) (ISO 13400-2) below it and
on [`uds_protocol`](https://crates.io/crates/uds_protocol) and
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)
for their shared vocabulary. The dependency edge runs from here to
`uds_services`, never the other way: `uds_services` never names a transport,
which is what keeps a binding additive at the application.

[`ARCHITECTURE.md`](ARCHITECTURE.md) has this crate's design, including its
seams to `simple_doip` and `uds_services` and the standard's own citations. How
the five crates fit together is the
[stack architecture](https://luminartech.github.io/uds_stack/architecture/index.html).
Where it and the sphinx-needs set under
[`docs/`](https://github.com/luminartech/uds_stack/tree/main/docs) at the
workspace root disagree, `docs/` wins.

## Scope

Of a UDS message, this crate reads what the standard requires it to: whether a
response is a positive `DiagnosticSessionControl` or a positive `ECUReset`,
which clause 8 keys TCP connection handling on, and a request's
suppressPosRspMsgIndicationBit and a response-pending negative response, which
decide whether the request is owed an answer. Decoding the rest is
[`uds_protocol`](https://crates.io/crates/uds_protocol)'s; dispatching
services, choosing negative response codes and the data identifiers themselves
are ISO 14229-1 behavior, and
[`uds_services`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_services)'s.
Addressing, the service primitives and the session state machine are
[`uds_session`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_session)'s
ISO 14229-2 vocabulary, used from there rather than redeclared here.

The crate is async without naming a runtime. `DoIpTransport` is generic over
`simple_doip`'s `DiagnosticEntity`, a trait with no I/O in it, and the sockets,
and whatever runtime drives them, are the entity's; a runtime dependency would
compromise the `no_std` build.

## Status

**Alpha.** It releases in lockstep with the rest of the stack, at one shared
version. `DoIpTransport<E: DiagnosticEntity, CONNECTIONS>` implements
`uds_services::UdsTransport`, and the server role is implemented: requests
and responses map onto `DoIP_Data` (ISO 14229-5:2022 REQ 4.3, REQ 4.4), and the
connection is closed after a positive `ECUReset` response (REQ 7.11) and after
the positive `DiagnosticSessionControl` response to a session change the server
says leaves its running software (REQ 7.9). A response longer than the entity
can carry is answered `responseTooLong`, because the server's buffers are sized
to the entity's `MAX_PDU`, and a request longer than the server decodes but
within what the entity holds is answered by UDS in ISO 14229-1's order: `0x11`
for a service the server does not support before `0x13` for a wrong length.
Time is the `DoIP` entity's own clock, which it reports through
`DiagnosticEntity::now`. The server role is tested end to end over loopback
against `simple_doip`'s own entity and tester, in the workspace's unpublished
`testing/doip-loopback` crate. The client role, `DoIpClientTransport` over any
`TesterConnection`, is tested there too: a `uds_client!` client reads from that
server, drives every service the sensor stages, survives the prescribed close by
reconnecting, and takes no late answer for a later request's. See the
[workspace README](https://github.com/luminartech/uds_stack#status) for how
this compares to the rest of the stack.

## Feature flags

None. The crate is `no_std` in every configuration, and its errors implement
`core::error::Error`. Enable `simple_doip`'s and `uds_protocol`'s features on those
crates directly.

## `no_std` support

`no_std` and allocation-free in its default configuration: no public type
contains a `Vec` or a `String`, and an inbound message borrows the receive
buffer. A bare-metal AURIX `TC4x` target is a qualification target for this
stack, which is why no feature here ever names a runtime.

## Usage

A transport wraps your `DoIP` entity, anything implementing
`simple_doip::service::DiagnosticEntity`, and is handed to a `uds_server!`
assembly as its `transport`:

```rust
use simple_doip::service::DiagnosticEntity;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;

fn transport_over<E: DiagnosticEntity, const CONNECTIONS: usize>(
    entity: E,
) -> DoIpTransport<E, CONNECTIONS> {
    DoIpTransport::new(entity, bench_reloads())
}
```

`CONNECTIONS` sizes the transport's table of which tester is on which
connection, and must be at least the entity's own
`DiagnosticEntity::CONNECTIONS`, which counts the reserve socket. A sensor
serving one tester runs `simple_doip`'s `Entity` with `MCTS` 1, so its table,
and the transport's, is 2: `DoIpTransport<Entity<'_, A, 1, MAX_MESSAGE>, 2>`. Its
`uds_server!` still declares `peers = 1`, the one tester it keeps a session for.

`bench_reloads()` gives the conventional `tP6` reload pair for a desk setup;
its documentation says why it is not a configuration for a vehicle.

### On bare metal

`uds_services::Server::step` takes `&mut self`, and this stack forbids `unsafe`,
so a server built in place in a `static` is reached in one of two ways. Where
one task owns it, a `static_cell` cell yields the `&'static mut` once. Where
anything else must reach it too, an interrupt handler or a second task, it
lives in a `critical_section::Mutex<core::cell::RefCell<..>>`.

A transport over `simple_doip`'s `entity::Entity` is built in place, as the
`static`'s initializer, where the acceptor is a `static` too:
`Entity::new_discovery_later`, `DoIpTransport::new` and
`uds_services::Server::new` are `const fn`s, so the cell is a
`static_cell::ConstStaticCell`, and the server is never on the stack. Its
initializer is an image in flash, copied to RAM at start-up. The entity is built
without discovery, because the identity it announces is known only at run time,
and discovery is started once the interface has an address, before the first
step:

```rust
let server = SERVER.take();
server
    .transport_mut()
    .entity_mut()
    .start_discovery(udp_socket, identity, seed);
```

Where the acceptor is created at run time, the entity borrows it then, so the cell
is a `static_cell::StaticCell`, filled with `init_with(|| Server::new(..))`.
`Entity::new` returns the entity by value, its buffers included, so the server is
built on the caller's stack and then copied into the cell: the stack needs room
for the whole server once, while it is built. The entity holds a receive buffer
and a transmit queue of `MAX_MESSAGE` bytes for each of its `MCTS` connections.

The integrator supplies, for the target:

- a `critical-section` implementation, such as `cortex-m`'s
  `critical-section-single-core` feature on a single-core Cortex-M;
- an `embassy-time` driver, which the entity's timers read: an
  `embassy_time_driver::Driver` registered with `time_driver_impl!`;
- an `embassy-time` timer queue, which wakes those timers: an executor's
  integrated queue, or `embassy-time-queue-utils`' generic one (its
  `generic-queue-N` feature) held in the driver and woken from its alarm.

The workspace's unpublished `testing/embedded-probe` assembles all of it for
bare metal: a `SysTick` driver with a generic queue, `cortex-m`'s critical
section, and the sensor's server built in place in a `ConstStaticCell`.

## Relationship to the standards

Neither license below grants any right in ISO 14229-5, which remains ISO's,
and no text of the standard is reproduced here or in `ARCHITECTURE.md`. See
["Relationship to the standards"](https://github.com/luminartech/uds_stack#relationship-to-the-standards)
in the workspace README for the fuller statement that applies to every crate
in this stack.

## Contributing

Pull requests, bug reports and questions are welcome; see
[`CONTRIBUTING.md`](https://github.com/luminartech/uds_stack/blob/main/CONTRIBUTING.md).
Security reports go through GitHub's private vulnerability reporting; see
[`SECURITY.md`](https://github.com/luminartech/uds_stack/blob/main/SECURITY.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option. Both files are the workspace's, shared by every crate here.
