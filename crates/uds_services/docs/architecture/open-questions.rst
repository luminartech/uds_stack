Open questions
==============

What is not yet settled, and why. This page holds no needs and contributes nothing to
``needs.json``; it is deleted when its last entry is answered.

Still open
----------

**1. How much of clause 8.7.6 belongs here?** Multiple concurrent requests with mixed
addressing is a clause 8.7 subclause, but occupancy of the diagnostic protocol instance is
the driver's. The two exceptions are the hard part: a functionally addressed
``TesterPresent`` with the suppress bit set must bypass the occupied resource, and a
request in the 0x00–0x0F range must abort an active service outside that range and start
the default session unless a programming session is active. Classifying a request as one
of those exceptions is arguably this crate's; acting on the classification is certainly
not. Proposed split, not yet agreed: this crate classifies, the driver acts.

**2. What does ``A_Mtype`` mean for this crate?** Clause 7.2 defines four formats —
diagnostics, remote, secure, and secure remote — and Figure 5's optional 0x38 and 0x39
checks exist to enforce the secure ones. Those are filed as caller-supplied checks in
``UDSSVC_ARCH_0011`` without an ``A_Mtype`` input to check against, which is at best
incomplete.

One of the two answers has closed. Leaving secure diagnostics out of scope is no longer
available: clause 7.2 and clause 16 are both this crate's under ``UDSSVC_ARCH_0001``, which
records the security sub-layer as in scope and not built. So the question is what carrying
``A_Mtype`` obliges, and whether the 0x38 and 0x39 checks stay caller-supplied once there
is something to check them against. It travels with clause 16 rather than ahead of it.

**3. What does a mid-response sink failure mean?** ``UDSSVC_ARCH_0017``'s sink can fail
after some bytes are written. Whether the binding must discard a partial response, and
whether this crate must therefore avoid writing until it can complete — which would
reintroduce a buffer, and with it the allocation question — is unsettled. It interacts with the sink vocabulary decision
recorded in ``UDSSVC_ARCH_0017``.

**4. How does a server that does not implement Authentication answer the 0x34 check?**
Figure 5 and Figure 6 both place an authentication check on the mandatory path.
``uds_protocol`` does not model the Authentication service (0x29), and most servers in
scope will not implement it. Presumably such a server passes the check unconditionally and
``Ctx``'s authentication input is what a server that *does* implement it supplies — but a
"mandatory check that is always true" deserves to be stated deliberately rather than
arrived at.

**5. Does ``RoutineControl`` need a distinct trait shape?** ``UDSSVC_ARCH_0007`` records
that Figure 5 excludes service identifier 0x31 from the sub-function stage, because its
sub-function is only meaningful together with the routine identifier. That means its
handler receives both parameters and decides 0x12 itself — a different contract from every
other sub-function service, and one this crate cannot centralise. Whether that is expressed
as a differently shaped trait or as documentation on an identically shaped one is open.

Open across the stack
---------------------

These cannot be settled in this repository alone. They are the substance of the brief
carried to the others.

**6. What does ``Ctx`` finally carry?** It is ``uds_session``'s struct now
(``UDSSVC_ARCH_0018``), and both crates have to agree on its contents before either writes
it. Authentication-succeeded is a straightforward addition, on Figure 5's mandatory path.
The question is the other three: this crate implements ``DiagnosticSessionControl``,
``SecurityAccess`` and ``Authentication``, so under ``UDSSVC_ARCH_0035`` it already holds the
active session, the security level and the authentication state, and two crates tracking one
fact is how they come to disagree. If nothing else owns them, ``Ctx`` reduces to the
addressing triple alone.

**7. ``uds_on_ip``'s client is entirely unimplemented.** Every method on it is
``todo!()``. The client surface of :doc:`client-surface` sits directly on it, so the client
half of this crate cannot be exercised end to end until that is real. Its *shape* is
settled enough to design against, which is why the elements are written; its behaviour is
not.
