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
  searched in order, so a later layer adds services but never replaces one.
- The record protocol: the server owns every response record, lends one to the
  handler it calls, and the handler fills it with `uds_record_copy`, which bounds
  every copy.
- The operation-status protocol: a handler that needs time answers response-pending
  and is called again on each poll until it settles, or told to cancel.
- Resets and session changes in three stages: every layer's check, every layer's
  prepare, the positive response, and only then every layer's execute.
- `edge-nal` sockets over the firmware's socket ABI, and the
  [`simple_doip`](https://crates.io/crates/simple_doip) entity and
  [`uds_on_ip`](https://crates.io/crates/uds_on_ip) transport they carry.
- With the `time-driver` feature, `embassy-time`'s clock over the firmware's.

What the firmware keeps is its own: the tables and handlers, a `Policy` (sessions,
timing, limits), the server's assembly with `uds_services::uds_server!`, and the two
entry points that start and poll it.

## The C contract

[`include/uds_c.h`](include/uds_c.h), shipped with the package, declares everything C
sees: the table types, the handler signatures, `uds_record_copy`, and the socket and
clock calls the firmware implements. Every `#[repr(C)]` type of the crate mirrors one
there under the same name. Both sides check the layouts for a 32-bit target, and a
host test compiles the header and compares every size and offset with Rust's.

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
}
```

The server is built in place as a `static`, with `uds_c::doip::transport`. At
start-up the firmware calls `App::validate()`, `uds_c::doip::start_discovery` and arms
the server's run future; on every pass of its loop it calls
`uds_c::execute_deferred(&LAYERS)` and then `uds_c::doip::poll_run`.

## Status

**Alpha.** Every service `uds_services` stages is served from the tables. It releases
in lockstep with the rest of the stack, at one shared version.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
