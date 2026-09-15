Open questions
==============

What is not yet settled, and why. This page holds no needs and contributes nothing to
``needs.json``; it is deleted when its last entry is answered.

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

**3. What does ``Ctx`` owe clause 7.4.1?** ISO 14229-1 clause 7.4.1 makes ``A_SA``,
``A_TA`` and ``A_TA_Type`` mandatory parameters of every application layer service
primitive; ``UDSSVC_ARCH_0015`` carries only the third.

This was a boundary question, defended on the ground that clause 8.7 reads only the third
and the crate went no further. ``UDSSVC_ARCH_0001`` withdrew that defence: clause 7 is this
crate's, so the parameters are its to carry or to drop deliberately — and dropping two of
three from what ``UDSSVC_ARCH_0019`` calls the application's service access point needs a
better argument than the one it had.

What keeps it open is the cost, not the boundary. ``UDSSVC_ARCH_0015`` declines a binding's
addressing triple because it is transport-shaped — ``uds_on_ip``'s carries a DoIP logical
address — so carrying ``A_SA`` and ``A_TA`` means finding a representation that is not. The
client half is evidence that it can be done: ``UDSSVC_ARCH_0022`` already carries each
responder's source address, for reasons of its own.

**4. What does ``A_Mtype`` mean for this crate?** Clause 7.2 defines four formats —
diagnostics, remote, secure, and secure remote — and Figure 5's optional 0x38 and 0x39
checks exist to enforce the secure ones. Those are filed as caller-supplied checks in
``UDSSVC_ARCH_0011`` without an ``A_Mtype`` input to check against, which is at best
incomplete.

One of the two answers has closed. Leaving secure diagnostics out of scope is no longer
available: clause 7.2 and clause 16 are both this crate's under ``UDSSVC_ARCH_0001``, which
records the security sub-layer as in scope and not built. So the question is what carrying
``A_Mtype`` obliges, and whether the 0x38 and 0x39 checks stay caller-supplied once there
is something to check them against. It travels with clause 16 rather than ahead of it.

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

**9. ``uds_on_ip``'s request context is missing a field.** It carries an addressing
triple, the active session and the security level; ``UDSSVC_ARCH_0015`` also requires the
authentication state, which Figure 5 places on the mandatory path above the session check.

**10. ``uds_on_ip``'s client is entirely unimplemented.** Every method on it is
``todo!()``. The client surface of :doc:`client-surface` sits directly on it, so the client
half of this crate cannot be exercised end to end until that is real. Its *shape* is
settled enough to design against, which is why the elements are written; its behaviour is
not.

**11. Is a shared addressing vocabulary wanted?** This crate needs only physical versus
functional (``UDSSVC_ARCH_0015``), and defines its own two-variant type to avoid depending
on a transport. ``uds_on_ip`` has a full ISO 14229-2 addressing triple, whose target
address type is the same distinction under a different name. Two types for one concept,
with a conversion in the adapter, is the current answer; whether the triple belongs in a
crate both can depend on is worth asking once rather than repeatedly — and question 3
raises the stakes on it.
