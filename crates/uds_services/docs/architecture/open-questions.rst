Open questions
==============

What is not yet settled, and why. This page holds no needs and contributes nothing to
``needs.json``; it is deleted when its last entry is answered.

Settled since the starting design
---------------------------------

Recorded because the reasoning is the useful part, and because three of these were settled
by reading the standard rather than by deciding anything.

**Per-service traits, or one trait with default methods?** Settled: per-service traits with
a declarative assembly macro, ``UDSSVC_ARCH_0012`` and ``UDSSVC_ARCH_0013``. The deciding
argument was not the one expected. Rust cannot ask whether a type implements a trait, so
per-service traits *require* an assembly step rather than merely benefiting from one — and
the alternative's real cost is that one trait must carry every service's associated types.

**Who validates message length?** Settled: ``uds_protocol`` detects, this crate maps to
0x13, ``UDSSVC_ARCH_0005``. The carve-out that came with it — an unmodelled service
identifier is 0x11 and not 0x13 — was not in the starting design.

**Does the dispatcher own the suppress-positive-response decision, or the caller?** Settled
by clause 8.7.5: the dispatcher. The pseudo-code evaluates the bit together with the
settled response code and the addressing mode in a single final gate, and the three rules
interact — the bit is ignored for negative responses, functional addressing silences five
codes, and a sent 0x78 overrides both. Splitting the decision between crate and caller
would mean splitting that gate, and no half of it is correct alone. ``UDSSVC_ARCH_0009``.

**Is the crate's scope statement the whole picture?** No, and this is the largest
correction the architecture has taken. The first draft of this set described a crate that
implements clause 8.7 and stopped there. It is also the point where an application meets
the stack — in both directions — and the client half was missing entirely: no elements, no
seams, no diagrams.

The evidence was already in the stack and had been read without being followed up.
``uds_on_ip``'s architecture names three portable surfaces — "defining identifiers,
implementing handlers, and **exchanging requests**" — and only the first two were modelled
here. Its client sends and receives ``&[u8]``, and its documentation states that
interpreting a negative response belongs to a higher layer, with no layer between it and
this one. The crate is named ``uds_services``, not ``uds_server``.

The fix keeps the two claims apart rather than merging them: ``UDSSVC_ARCH_0001`` still
bounds what the crate *implements* to clause 8.7, and ``UDSSVC_ARCH_0019`` states
separately what it *is*. A scope statement stretched to cover the client surface would
have stopped being checkable. :doc:`client-surface` is the half that was missing.

**Where are the identifier types declared, and what must they provide?** Settled:
free-standing, role-neutral traits, bound as an associated type by a service trait on the
server and inferred from the arguments at a client call site. ``UDSSVC_ARCH_0014`` carries
the shape.

Three constraints decided it rather than taste. Client code implements no service trait
and must still name the type; the vocabulary must supply each record's extent, because it
is not on the wire; and it compiles into both firmware and host tooling, so it must be
``no_std`` without ``alloc``. A grouping trait over all three identifier kinds satisfies
the first but forces a server with no routines to name a routine type, so it was rejected.

The record hook is ``split_record``, delegating to the record type's own ``Decode``, not a
``record_len`` returning a number — see ``UDSSVC_ARCH_0026``. That was the ergonomic
question that turned out to have a better answer than either option originally on the
table: the stack's codec already splits a value off the front of a buffer and returns the
remainder, and ``Encode::encoded_size`` is derived by construction precisely because
hand-maintained sizes were "the bug class every migrated consumer had".

**Is the client surface asynchronous, and what does that cost?** Settled: a sans-io
synchronous core with an asynchronous client layered over it, generic over a transport
trait this crate declares (``UDSSVC_ARCH_0028``, ``UDSSVC_ARCH_0029``).

The question was framed wrongly at first. It was posed as async-or-not, weighed against
``no_std``; but ``async`` is a language feature needing no runtime in a library, and what
would break an embedded build is a tokio dependency, which a transport-trait-generic
client does not have. ``UDSSVC_ARCH_0030`` states the assumption plainly: an executor
exists everywhere, ``embassy`` included, and this crate depends on none.

What decided the *shape* was the direction of control. Everything below is asynchronous
already — ``simple_doip``'s and ``uds_on_ip``'s client and server features each imply
``std`` and tokio — so a client that did not await would hand the application a buffer,
make it call the binding, and take the bytes back. A server needs no equivalent because it
is called rather than calling.

**It also settled a server-side question nobody had asked.** Once a runtime is assumed the
handler seam can be asynchronous too (``UDSSVC_ARCH_0016``), which removes one of the two
constraints this design placed on the binding's driver: a slow handler yields, rather than
having to be run somewhere the driver can continue past. That was machinery every
integrator would otherwise have built. The seam it changes, ``uds_on_ip``'s
``RequestHandler``, is declared and unimplemented, so it costs a signature now and a
migration later.

**Is the negative-response code set complete?** No, and this is worth recording as a
correction rather than a settlement. The starting design listed eight codes. Figures 5 and
6 add 0x21, 0x24, 0x34, 0x38 and 0x39 — one of them, 0x34, on the *mandatory* path, ahead
of the session check. ``UDSSVC_ARCH_0006`` and ``UDSSVC_ARCH_0011`` carry the full set. A
design that had been implemented against the eight would have had a mandatory check
missing and its ordering wrong.

Still open
----------

**1. Two server seams already exist.** ``simple_doip``'s bare-metal entity exports a
``fn(&[u8], &mut [u8]) -> i32`` request callback in production code — the same byte seam
``uds_on_ip`` declares, one layer further down. Either that callback is the canonical seam
for ``no_std`` targets and this crate should target it too, or it is a bare-metal
convenience that does not compose with this path. It blocks the server story rather than
decorating it.

**2. How much of clause 8.7.6 belongs here?** Multiple concurrent requests with mixed
addressing is a clause 8.7 subclause, but occupancy of the diagnostic protocol instance is
the driver's. The two exceptions are the hard part: a functionally addressed
``TesterPresent`` with the suppress bit set must bypass the occupied resource, and a
request in the 0x00–0x0F range must abort an active service outside that range and start
the default session unless a programming session is active. Classifying a request as one
of those exceptions is arguably this crate's; acting on the classification is certainly
not. Proposed split, not yet agreed: this crate classifies, the driver acts.

**3. Does ``Ctx`` owe clause 7.4.1 more than it carries?** ISO 14229-1 clause 7.4.1 makes
``A_SA``, ``A_TA`` and ``A_TA_Type`` mandatory parameters of every application layer
service primitive; ``UDSSVC_ARCH_0015`` carries only the third. The defence is that clause
8.7 reads only the third and ``UDSSVC_ARCH_0001`` bounds the crate to clause 8.7. The
tension is that ``UDSSVC_ARCH_0019`` makes this crate the application's service access
point, and an access point that drops two of three mandatory parameters is a lossy one.
Note the client side already answers the opposite way (``UDSSVC_ARCH_0022``).

**4. What does ``A_Mtype`` mean for this crate?** Clause 7.2 defines four formats —
diagnostics, remote, secure, and secure remote — and Figure 5's optional 0x38 and 0x39
checks exist to enforce the secure ones. Those are filed as caller-supplied checks in
``UDSSVC_ARCH_0011`` without an ``A_Mtype`` input to check against, which is at best
incomplete. Whether this crate carries ``A_Mtype`` or leaves secure diagnostics out of
scope entirely has not been decided.

**5. What does a mid-response sink failure mean?** ``UDSSVC_ARCH_0017``'s sink can fail
after some bytes are written. Whether the binding must discard a partial response, and
whether this crate must therefore avoid writing until it can complete — which would
reintroduce a buffer, and with it the allocation question — is unsettled. It interacts
with question 6.

**6. Who produces ``responseTooLong`` (0x14)?** It appears in ``uds_protocol``'s permitted
codes for ``ReadDataByIdentifier`` but in neither Figure 5 nor Figure 6. The natural
producer is whoever discovers the response will not fit, which is the sink — but a sink
that is merely full is not obviously distinguishable from one that failed, and the maximum
response length is a transport property the binding knows and this crate does not.

**7. How does a server that does not implement Authentication answer the 0x34 check?**
Figure 5 and Figure 6 both place an authentication check on the mandatory path.
``uds_protocol`` does not model the Authentication service (0x29), and most servers in
scope will not implement it. Presumably such a server passes the check unconditionally and
``Ctx``'s authentication input is what a server that *does* implement it supplies — but a
"mandatory check that is always true" deserves to be stated deliberately rather than
arrived at.

**8. Does ``RoutineControl`` need a distinct trait shape?** ``UDSSVC_ARCH_0007`` records
that Figure 5 excludes service identifier 0x31 from the sub-function stage, because its
sub-function is only meaningful together with the routine identifier. That means its
handler receives both parameters and decides 0x12 itself — a different contract from every
other sub-function service, and one this crate cannot centralise. Whether that is expressed
as a differently shaped trait or as documentation on an identically shaped one is open.

Open across the stack
---------------------

These cannot be settled in this repository alone. They are the substance of the brief
carried to the others.

**9. ``uds_on_ip``'s request context is missing two fields.** It carries an addressing
triple, the active session and the security level. ``UDSSVC_ARCH_0015`` also requires the
authentication state and whether a response-pending has been sent. Without the second,
clause 8.7.5's override cannot be applied and a functionally addressed request that took
too long is answered with silence where the standard requires a final negative response.

**10. ``uds_session`` must expose that a 0x78 has been sent.** It owns the timer and the
decision, so it is the only crate that knows. Today that state is internal.

**11. ``uds_on_ip``'s client is entirely unimplemented.** Every method on it is
``todo!()``. The client surface of :doc:`client-surface` sits directly on it, so the client
half of this crate cannot be exercised end to end until that is real. Its *shape* is
settled enough to design against, which is why the elements are written; its behaviour is
not.

**12. Is a shared addressing vocabulary wanted?** This crate needs only physical versus
functional (``UDSSVC_ARCH_0015``), and defines its own two-variant type to avoid depending
on a transport. ``uds_on_ip`` has a full ISO 14229-2 addressing triple, whose target
address type is the same distinction under a different name. Two types for one concept,
with a conversion in the adapter, is the current answer; whether the triple belongs in a
crate both can depend on is worth asking once rather than repeatedly — and question 3
raises the stakes on it.
