# `uds_services` — starting design

The state a design conversation reached before any code existed. It is a
**starting point for iteration**, not a specification: it records what was
decided, why, and — most usefully — what was left open.

> **Status.** Pre-implementation. This crate flows through the same process as
> the rest of the stack: a prototype to retire technical risk, then requirements
> and architecture authored in sequence, then development closing the gap
> between the two. This document belongs to the prototype phase. It is not the
> SWE.2 architecture and carries no requirement IDs.
>
> **Requirements are authored from the standard, not from this document.** A
> requirement set fitted to what a prototype happens to do agrees with the code
> by construction, and the gap-closing step then finds nothing. Read what
> follows as evidence about what is *buildable*.

> **Spec citations.** A numbered locator appears here only if it was checked
> against a copy of the standard. Every one below was verified on 2026-09-10.
> If you cannot check a citation, delete it rather than soften it.

---

## 1. What this crate is

**ISO 14229-1:2020 clause 8.7, "Server response implementation rules."**

That clause is the dispatch-and-negative-response state machine a UDS server
must implement: which validation steps run in which order, which negative
response code each failure produces, and — the part most implementations get
wrong — when the correct answer is silence rather than a negative response.

Its subclauses are the crate's scope:

| Clause | Content |
| --- | --- |
| 8.7.1 | General definitions and legend (`suppressPosRspMsgIndicationBit`, PosRsp, NegRsp, NoRsp, ALL / At least 1 / None) |
| 8.7.2 | General server response behaviour — the mandatory validation sequence (Figure 5) |
| 8.7.3 | Requests **with** a SubFunction: general, physically addressed, functionally addressed |
| 8.7.4 | Requests **without** a SubFunction: the same three |
| 8.7.5 | Pseudo-code example of server response behaviour |
| 8.7.6 | Multiple concurrent requests with physical and functional addressing |

Clause 8.7.2 classifies validation steps as *mandatory*, *optional*, or
*manufacturer/supplier specific*, which is the seam where a caller's own checks
attach.

### What it is not

- **Not a codec.** Message encode/decode is `uds_protocol`. This crate
  dispatches over those types and never re-derives the wire format.
- **Not a session layer.** Timers, `tS3`, and response-pending belong to
  `uds_session`. See [§6](#6-what-this-crate-does-not-own).
- **Not transport-aware.** See [§4](#4-transport-integration). A
  `ReadDataByIdentifier` handler that knows how to fetch an identifier has
  nothing to say about IP, and folding a transport in would make the ergonomic
  layer — the part application authors actually touch — the one piece of the
  stack that cannot follow to CAN.

## 2. Position in the stack

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

`uds_protocol` (ISO 14229-1 message definitions) feeds this crate and both
bindings.

The organising rule across the stack is one crate per ISO document, so "does
this belong here?" is answered by asking which document specifies the
behaviour. For this crate the answer is narrow and precise: clause 8.7 and
nothing else.

## 3. The typed dispatch

### 3.1 The application owns the identifiers

Data identifiers, routine identifiers, and most DTCs are vehicle-manufacturer
specific — ISO 14229-1 fixes ranges and a small set of standardised values, not
the catalogue. The library therefore cannot own those enumerations. The
application supplies them as associated types:

```rust
pub trait UdsServer {
    type Did:  DataIdentifier;      // application's enum
    type Rid:  RoutineIdentifier;
    type Sess: SessionType;
}

pub trait ReadDataByIdentifier: UdsServer {
    fn read<W: embedded_io::Write>(
        &mut self,
        did: Self::Did,
        out: &mut W,
    ) -> Result<(), Nrc>;
}
```

This is what makes the compiler the completeness check the design is aiming
for. Adding a variant to the application's `Did` enum breaks its own exhaustive
`match`, so a newly defined identifier cannot silently go unhandled. A service
the application does not implement at all answers `serviceNotSupported`
automatically.

The goal, stated plainly: **an application using this stack should not need to
care about UDS much at all.** A server implements the services it supports over
a typed set of messages, and everything else just works.

### 3.2 The negative-response rules are the value

Anyone can write a match on a service identifier. What is worth centralising is
clause 8.7's validation order and its response/silence rules, because they are
easy to get subtly wrong and the failure is invisible in testing against a
cooperative client.

Codes this crate selects, all verified against Annex A:

| NRC | Name | Raised when |
| --- | --- | --- |
| `0x11` | `serviceNotSupported` | the service trait is not implemented |
| `0x12` | `SubFunctionNotSupported` | sub-function outside the implemented set |
| `0x13` | `incorrectMessageLengthOrInvalidFormat` | length/format check fails |
| `0x22` | `conditionsNotCorrect` | precondition not met |
| `0x31` | `requestOutOfRange` | identifier outside the supported set |
| `0x33` | `securityAccessDenied` | security precondition not met |
| `0x7E` | `SubFunctionNotSupportedInActiveSession` | supported, wrong session |
| `0x7F` | `serviceNotSupportedInActiveSession` | supported, wrong session |

### 3.3 Silence is a response, and addressing decides it

The rule most likely to be implemented wrongly. On a **functionally addressed**
request, clause 8.7.3.3 and 8.7.4.3 require the server to send **no response at
all** for `SNS`, `SNSIAS`, `SFNS`, `SFNSIAS` and `ROOR` — where the same failure
on a **physically addressed** request produces a negative response.

The reasoning is that a functional request reaches every server on the bus, and
a bus-wide chorus of "I don't support that" would be worse than useless.

Two corollaries from the same subclauses:

- `suppressPosRspMsgIndicationBit` is **ignored for any negative response**. It
  suppresses positive responses only.
- NRC `0x78` overrides both suppressions. Annex A is explicit: when `0x78` is
  used the server shall always send a final response, independent of
  `suppressPosRspMsgIndicationBit` *or* the functional-addressing suppression
  above.

This is why [`Ctx`](#5-the-context-a-handler-receives) must carry the addressing
mode. A dispatcher that does not know how the request was addressed cannot apply
clause 8.7 correctly, and the bug will not show up against a physically
addressed test client.

### 3.4 Open: per-service traits plus a macro, or one trait with defaults

Unresolved, and the first real design decision.

**One trait, default methods.** Every service is a method on `UdsServer`
defaulting to `Err(Nrc::ServiceNotSupported)`; the application overrides what it
supports. Simple, no proc-macro dependency, but one large trait and no
compile-time signal distinguishing "deliberately unsupported" from "forgot".

**Per-service traits plus `uds_server! { Ecu: ReadDataByIdentifier, ... }`.**
Composable and independently testable, and the macro extends the completeness
signal from the identifier level up to the *service* level — the assembly list
is explicit, so omitting a service is visible. Costs a proc-macro in a stack
that currently has none.

The tie-breaker is probably whether service-level completeness is worth as much
as identifier-level completeness. It is not obvious that it is.

## 4. Transport integration

This crate owns the **typed service traits**. It does not own the byte seam.

A byte seam is necessarily transport-shaped — it is where a binding hands off —
so each binding declares its own, and this crate implements whichever one its
enabled feature selects:

```toml
[features]
doip  = ["dep:uds_on_ip"]
docan = ["dep:uds_on_can"]
```

`uds_on_ip`'s seam, for reference:

```rust
pub trait RequestHandler {
    fn handle<W>(&mut self, ctx: &Ctx, request: &[u8], out: &mut W)
        -> Result<Outcome, W::Error>
    where W: embedded_io::Write;
}
```

so this crate provides, behind `doip`, a blanket
`impl RequestHandler for T where T: UdsServer`.

The dependency edge points one way — `uds_services → uds_on_ip`, optional — and
`uds_on_ip` stays ignorant of what a service is. This is the `tower`/`hyper`
arrangement, and it is what lets a typed server move to CAN without change.

**What is not portable, stated so nobody is surprised:** the diagnostic
*conversation* moves between transports; *connection setup* does not. DoIP has
TCP connections, routing activation, and vehicle identification; CAN has none of
them. Defining identifiers and implementing handlers is portable. Establishing
and configuring the link is a real seam in the application.

## 5. The context a handler receives

```rust
pub struct Ctx {
    pub ai: Ai,                  // who sent it, and how it was addressed
    pub active_session: u8,      // ISO 14229-1 sub-function value
    pub security_level: u8,
}
```

Session and security arrive as **raw sub-function bytes**, passed through
uninterpreted by the binding. Naming the sessions here would mean this crate
deciding what `DiagnosticSessionControl`'s sub-functions mean, which is
`uds_protocol`'s to define and the application's to choose.

Crucially, this crate takes session and security as **parameters rather than a
dependency**. It never depends on `uds_session`. That keeps it usable under any
binding, and avoids a hard dependency on a crate that is currently private.

`Outcome` rather than `Result` for the dispatch result: a negative response is a
normal outcome expressed in the written bytes, not an error. Genuine transport
failures are the binding's concern and never reach a handler.

## 6. What this crate does not own

Explicit because these were argued and settled, and the boundaries are the
easiest thing to erode.

| Concern | Owner | Why not here |
| --- | --- | --- |
| Message encode/decode | `uds_protocol` | It is a codec; this is policy over it |
| `tP_Client`, `tS3`, session state | `uds_session` | ISO 14229-2 |
| **Deciding to send NRC `0x78`** | `uds_session` | See below |
| `tP2_Server` / `tP2*_Server` | `uds_session` | ISO 14229-2 timers |
| `tP4_Server` | *this crate*, but not as a timer | ISO 14229-2 types it a **performance requirement** on the application, not something to run |
| The byte seam | the binding | Transport-shaped ([§4](#4-transport-integration)) |
| A_PDU framing, TCP handling, `0x8004` | `uds_on_ip` | ISO 14229-5 |

**Response-pending is the subtle one.** Emitting `0x78` before `tP2_Server`
expires cannot be this crate's job: a handler that is still working is by
definition not returning, so it cannot also be watching a clock — and the clock
is an ISO 14229-2 timer. So `uds_session` owns the timer and decides, the
binding transmits, and this crate is simply still running.

Two consequences that constrain the *binding's* server driver, not this crate:
dispatch must not block the loop draining session actions, and because
`RequestHandler` is synchronous, a slow handler has to run somewhere the driver
can continue past.

## 7. `no_std` and alloc-freedom

`no_std`, no allocation, designed in from the start rather than deferred —
alloc-freedom cannot be retrofitted, because the signatures that make an API
alloc-free are the signatures callers depend on.

Concretely: a handler writes its response into a caller-supplied
`embedded_io::Write` sink rather than returning a `Vec`. No public type carries
a `Vec` or a `String`.

The sink is a **generic parameter, not `dyn`**, which costs object safety —
there is no `Box<dyn UdsServer>`, and a driver is generic over the handler type
instead. For a crate that must build without `alloc`, where boxing is
unavailable anyway, that is the right trade, but it is a deliberate one and
should be revisited if a dynamic registry is ever wanted.

Verify with a bare-metal target rather than `--no-default-features` on a hosted
one; only a `*-none` target proves `std` has not crept back in through a
dependency.

## 8. Traceability

`uds_session` has settled the apparatus; adopt it rather than inventing a second.

- Requirements are sphinx-needs `llr` directives carrying `:id:`, `:status:`,
  `:integrity_level:`, `:target_level:`, `:origin:`, `:source:`, `:tags:`.
- `:source:` holds spec locators, semicolon-separated — the same format used in
  prose here, so citations port directly.
- `impl` and `test` needs are generated from source annotations, so code links
  to requirement IDs rather than being matched up by hand.
- `needs.json` is a published interface consumed across repositories through
  `needs_external_needs`.

Two consequences:

1. This crate needs its own ID prefix. `uds_session` pins
   `^UDSS_LLR_\d{4}$|^UDSS_(IMPL|TEST)_[A-Z0-9_]+$`.
2. **This crate has the highest derived fraction in the stack**, and that is the
   thing to watch. Clause 8.7 gives the NRC rules and the validation order, but
   says nothing about trait design, associated types, or macros. Most of
   `uds_on_ip`'s requirements are *transcribed* from ISO 14229-5 and carry
   inherent ordering evidence; a large share of this crate's will be *derived*,
   and derived requirements must record the reasoning that produced them,
   because that reasoning cannot be reconstructed afterwards.

## 9. Open questions

1. **Per-service traits plus a macro, or one trait with default methods?**
   ([§3.4](#34-open-per-service-traits-plus-a-macro-or-one-trait-with-defaults))
2. **Is `Ctx` sufficient?** It carries addressing, session, and security. If
   clause 8.7 gating turns out to need richer state, the seam widens and both
   this crate and every binding change. Worth settling early.
3. **Who validates message length?** `incorrectMessageLengthOrInvalidFormat`
   (`0x13`) is a clause 8.7 outcome, but length is a property of the encoding,
   which is `uds_protocol`'s. Probably: `uds_protocol` detects, this crate maps
   the failure to the NRC. Needs confirming against the decode error taxonomy.
4. **Two server seams already exist.** `simple_doip`'s bare-metal entity
   exports `on_uds_request: fn(&[u8], &mut [u8]) -> i32` in production code —
   the same byte seam `uds_on_ip` declares, one layer further down. Either that
   callback is the canonical seam for `no_std` targets and this crate should
   target it too, or it is a bare-metal convenience that does not compose with
   this path. Unresolved, and it blocks the server story rather than decorating
   it.
5. **How much of 8.7.6 belongs here?** Multiple concurrent requests with mixed
   physical and functional addressing is a clause 8.7 subclause, but
   concurrency is the binding's. The split is not obvious.
6. **Does the dispatcher own the `suppressPosRspMsgIndicationBit` decision, or
   the caller?** It interacts with the functional-addressing silence rules and
   with `0x78`, so centralising it is attractive — but it is a per-request
   client instruction, not a server policy.
