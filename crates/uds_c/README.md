# uds_c

A [`uds_stack`](https://github.com/luminartech/uds_stack) UDS server for C firmware:
ISO 14229-1 services over ISO 13400-2 `DoIP`, with the firmware's services described as
tables of C handlers.

## Where this fits

The protocol crates of the stack each own a standard. This crate owns none: it is the
seam between them and firmware written in C. The firmware provides its tables, its
handlers, a socket ABI over its own TCP/IP stack and a millisecond clock; this crate
provides everything between those and a running server:

- `TableApp`, an application implementing every service trait of
  [`uds_services`](https://crates.io/crates/uds_services) over the tables, in layers
  searched in order, so a later layer adds services but never replaces one. A DTC,
  communication control or download service that no layer serves is answered
  `serviceNotSupported` (0x11).
- The record protocol: the server owns every response record, lends one to the
  handler it calls, and the handler fills it with `uds_record_copy`, which bounds
  every copy, or with `uds_record_copy_16_be`, `_32_be` and `_64_be` for an integer
  in big-endian order.
- The DTC services over a DTC memory a layer keeps: the server reads each DTC's number,
  status and snapshot record and encodes the reports itself, and the memory updates each
  status with the ISO 14229-1 Annex D transitions the crate exports to C, so firmware
  keeps its DTCs and their storage but restates none of the standard.
- The operation-status protocol: a handler that needs time answers response-pending
  and is called again on each poll until it settles, or told to cancel.
- Resets and session changes in three stages: every layer's check, every layer's
  prepare, the positive response, and only then every layer's execute; a change not
  made after all is abandoned by every layer that prepared it.
- `edge-nal` sockets over the firmware's socket ABI, and the
  [`simple_doip`](https://crates.io/crates/simple_doip) entity and
  [`uds_on_ip`](https://crates.io/crates/uds_on_ip) transport they carry.
- With the `time-driver` feature, `embassy-time`'s clock over the firmware's. Without
  it, the image links an `embassy-time` driver of its own: the security delays read
  `embassy_time::Instant::now`.

What the firmware keeps is its own: the tables and handlers, a `Policy` (sessions,
timing, limits), the server's assembly with `uds_services::uds_server!`, and the two
entry points that start and poll it.

## The C contract

[`include/uds_c.h`](include/uds_c.h), shipped with the package, declares everything C
sees: the table types, the handler signatures, `uds_record_copy`, and the socket and
clock calls the firmware implements. Every `#[repr(C)]` type of the crate mirrors one
there under the same name. Both sides check the layouts for a 32-bit target, and a
host test compiles the header and compares every size and offset with Rust's, so
`cargo test` needs a C11 compiler: `$CC`, or `cc` on the `PATH`.

Every call, in both directions, belongs to the one execution context that polls the
server. The handler being served and the transition awaiting its response are each
kept in a `static`, so an image runs one server built on this crate.

## Using it

```rust,ignore
unsafe extern "C" {
    static platform_layer: UdsLayer;
    static oem_layer: UdsLayer;
}

static LAYER_LIST: [&UdsLayer; 2] = unsafe { [&platform_layer, &oem_layer] };
static LAYERS: Layers = unsafe { Layers::new(&LAYER_LIST) };

struct Ecu;

impl uds_c::Policy for Ecu {
    const LAYERS: &'static Layers = &LAYERS;
    const SESSIONS: Sessions = Sessions::NONE
        .with(DiagnosticSessionType::DefaultSession)
        .with(DiagnosticSessionType::ExtendedDiagnosticSession);
    fn timing(_session: DiagnosticSessionType) -> SessionTiming {
        TIMING
    }
}

type App = uds_c::TableApp<Ecu>;
type Transport = uds_c::doip::Transport<2048, 1>;

uds_server! {
    App: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = Transport,
    peers = 1,
    services = EcuServices,
    server = EcuServer,
    serves = App::serves,
}
```

The server is built in place in a `uds_c::Firmware` `static`, with
`uds_c::doip::transport`. A `static` needs its run future's type named, which takes the
nightly `type_alias_impl_trait` feature in the firmware's crate; this crate itself
builds on stable. At start-up the firmware calls `App::validate()` and hands the
`Validated` proof it returns to `Firmware::arm`, whose closures start discovery with
`uds_c::doip::start_discovery` and build the run future; on every pass of its loop it
calls `Firmware::poll(&LAYERS, run)`, which makes the resets and session changes whose
responses have been sent, then polls the server once. `Firmware` is the one way to drive
the server. Both calls are `unsafe`: the firmware promises to make them from that loop
alone.

## Status

**Alpha.** Every service `uds_services` stages is served from the tables. It releases
in lockstep with the rest of the stack, at one shared version.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
