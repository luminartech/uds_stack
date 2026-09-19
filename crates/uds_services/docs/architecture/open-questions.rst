Open questions
==============

What is not yet settled, and why. This page holds no needs and contributes nothing to
``needs.json``; it is deleted when its last entry is answered.

Still open
----------

**1. Acting on clause 8.7.6's classification.** The question used to be how much of clause
8.7.6 belongs here at all, and proposed a split — this crate classifies, the driver acts —
across what was then a crate boundary. Both halves have since landed on the same side.
``UDSSVC_ARCH_0040`` made this crate the driver, so there is no boundary left to split
across, and the classification is here: ``ServiceSet::is_concurrent_exception`` answers it,
and ``UDSSVC_ARCH_0017``'s small concurrent buffer exists so that a message arriving
mid-service can be received and classified at all. The argument that settled ownership is
unchanged and was the strong one: the second exception is predicated on "if a server
supports services in the range of 0x00 to 0x0F", which only ``UDSSVC_ARCH_0013``'s assembly
list knows, so classification is not merely *arguably* this crate's — it is available
nowhere else.

**What remains open is acting on it**, and the gap is visible in the driver today. A
functionally addressed ``TesterPresent`` with the suppress bit set must bypass the occupied
diagnostic protocol instance, and a request in the 0x00–0x0F range must abort an active
service outside that range and start the default session unless a programming session is
active. The first is not performed. The second cannot arise: the clause predicates it on
the server supporting a service in that range, ``uds_server!`` can assemble none — the
lowest identifier in ``__uds_sids!`` is 0x10, that range being OBD territory
``uds_protocol`` does not model — and ``is_concurrent_exception`` therefore no longer
carries an arm for it. The arm returns with the first service in that range, which cannot
be added without editing the same macro. Nor is the negative case: anything arriving mid-service that
is *not* an exception is occupancy and owes ``busyRepeatRequest`` (0x21), and a
``DataTooLong`` on the concurrent buffer owes the same — the driver's concurrent arm
currently re-arms the deadline and does nothing else. No caller in ``src/`` invokes
``is_concurrent_exception`` at all; the only call site in the repository is
``tests/composition.rs``, which exercises it to prove the classifier is reachable. So the
classification exists and is tested, and nothing in the running server consults it. See
:doc:`dispatch`.

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

**3. Must a transport be told that a response was abandoned?** ``UDSSVC_ARCH_0017``'s sink
can still fail after some bytes are written; what has changed is who has to agree about it.
The question used to ask whether *the binding* must discard a partial response, and whether
this crate must therefore avoid writing until it could complete — which would have
reintroduced a buffer and with it the allocation question. Neither applies now. This crate
owns both the buffer and the transport, and a partial response is discarded by the only
means that matters: **it is not submitted**. Nothing reaches ``uds_session`` or the
transport until the handler has returned and the outcome says bytes are to be sent, so a
failed response never leaves the crate and the buffer is simply overwritten by the next
one. The buffer the old question feared reintroducing was already there.

What is left is narrower and is a property of the seam rather than of the sink: whether a
transport must be *told* that a response it was expecting will not arrive. On
``UDSSVC_ARCH_0029`` it need not be, because it is never made aware of an impending
response — ``t_data_req`` is the first it hears of one. The question stays open only against
a future transport that acquires a resource in anticipation of a send.

**4. How does a server that does not implement Authentication answer the 0x34 check?**
Figure 5 and Figure 6 both place an authentication check on the mandatory path.
``uds_protocol`` does not model the Authentication service (0x29), and most servers in
scope will not implement it. Presumably such a server passes the check unconditionally and
``Ctx``'s authentication input is what a server that *does* implement it supplies — but a
"mandatory check that is always true" deserves to be stated deliberately rather than
arrived at.

**Moot for every server in scope today, and that is not the same as answered.** Service 0x29
has no ``uds_protocol`` message type in either direction, so it decodes to
``Request::Other`` and settles ``serviceNotSupported`` (0x11) — a server that wants
Authentication *cannot implement it*, and there is accordingly no ``Authentication`` trait.
So for every server this crate can currently assemble, the check is unconditionally true and
nothing turns on it. What is deferred rather than settled is the deliberate statement: a
mandatory check that is always true should be recorded as such, with the reason, at the
point where 0x29 becomes implementable. ``Ctx``, which this question named as the carrier of
the authentication input, no longer exists (question 6).

Retired
-------

Kept rather than deleted, because a question's answer is only evidence alongside what was
asked and why. The numbering is not reused.

**5. Does ``RoutineControl`` need a distinct trait shape? — yes, three methods.** It asked
whether Figure 5's exclusion of service identifier 0x31 from the sub-function stage should
be expressed as a differently shaped trait or as documentation on an identically shaped one.
``RoutineControl`` carries ``start``, ``stop`` and ``results``, one per clause 13.2
sub-function. ``UDSSVC_ARCH_0007`` records the reason: 0x31 is the only service whose
*handler* decides ``subFunctionNotSupported`` (0x12), because it is the only one the
centralised sub-function stage does not run for, and a single method taking a sub-function
byte would leave a signature indistinguishable from every service whose 0x12 is settled
before the handler is reached. Documenting the asymmetry was the alternative; making it
visible in the surface is what was built.

**6. What does ``Ctx`` finally carry? — nothing; there is no ``Ctx``.** It asked what the
struct's final field list would be, and was filed as open *across the stack* because
``uds_session`` owned the type at the time and both crates had to agree before either wrote
it. Neither premise survived. ``UDSSVC_ARCH_0018`` removed the seam, so no cross-crate
agreement was owed; and the three fields the question turned on — active session, security
level, authentication state — all resolved the same way, which is the way the question
suspected: this crate implements the services that hold them, so reading them back from a
struct it built from state it owns is a copy rather than an input. What remained was the
addressing triple alone, and a struct wrapping one ``Ai`` is a rename. ``UDSSVC_ARCH_0015``
carries the full reasoning and the field table.

Open across the stack
---------------------

These cannot be settled in this repository alone. They are the substance of the brief
carried to the others.

**7. ``uds_on_ip``'s client is entirely unimplemented.** Every method on it is
``todo!()``. The client surface of :doc:`client-surface` sits directly on it, so the client
half of this crate cannot be exercised end to end until that is real. Its *shape* is
settled enough to design against, which is why the elements are written; its behaviour is
not.
