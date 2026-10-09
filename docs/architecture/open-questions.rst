Open questions
==============

What is not yet settled, and why. This page holds no needs and contributes nothing to
``needs.json``; it is deleted when its last entry is answered.

Still open
----------

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

**8. How is ``DataTransfer`` staged?** It is the one service with a trait and no stage, so
a listed one answers ``serviceNotSupported`` (0x11). Four design problems block it, and
each needs an answer before a stage is written:

* Figure 31 checks address and size (0x31) before security (0x33), but address validity is
  the handler's to decide.
* ``RequestFileTransfer``'s positive response needs file sizes that ``pipeline::begin``
  cannot return.
* ``State`` holds no transfer.
* 15.2.3.2's ``maxNumberOfBlockLength`` includes the service identifier and the block
  sequence counter, and ``DataTransfer::MAX_BLOCK_LENGTH`` excludes them.

Tracked as #50.

Retired
-------

Kept rather than deleted, because a question's answer is only evidence alongside what was
asked and why. The numbering is not reused.

**1. Acting on clause 8.7.6's classification — the driver refuses, the keep-alive
bypasses.** It asked how much of clause 8.7.6 belonged here, then, once ``UDSSVC_ARCH_0040``
made this crate the driver and ``ServiceSet::is_concurrent_exception`` the classifier, why
nothing in ``src/`` consulted it: a message arriving mid-service was received into the
concurrent buffer and dropped. The driver now acts on it. A message arriving while a
service is in progress finds the one protocol instance occupied (ISO 14229-1:2020 8.7.6)
and is answered ``busyRepeatRequest`` (0x21, Annex A) from its service identifier, whether
it was physically or functionally addressed and whether or not it fit the concurrent
buffer; the service in progress continues. The functionally addressed ``3E 80`` is the
exception: it is indicated to the session layer as keep-alive (``UDSS_LLR_0095``,
``UDSS_LLR_0096``) and neither dispatched nor answered.

What it took was a session-layer kind, not only a driver arm. Sent as a solicited final
response to the client whose service is in progress, a 0x21 would have answered that
service by addressing (``UDSS_LLR_0106``): stopped its ``tP2_Server`` and ended it on
confirmation. ``UDSS_LLR_0187``'s busy refusal answers no service, and ``UDSS_LLR_0108`` now
governs only the requests a caller indicates. One association serves the one client, so a
0x78 that comes due while a 0x21 is unconfirmed is refused, and the driver resubmits it on
the confirmation that frees the association; a 0x21 refused for the same reason is dropped,
Annex J Figure J.2's other branch. The OBD-range exception stays absent for the reason the
classifier's own doc gives: no assemblable service reaches it.

**5. Does ``RoutineControl`` need a distinct trait shape? — yes, three methods and a
lookup keyed by routine.** It asked whether Figure 5's exclusion of service identifier 0x31
from the sub-function stage should be expressed as a differently shaped trait or as
documentation on an identically shaped one. ``RoutineControl`` carries ``start``, ``stop``
and ``results``, one per clause 14.2 sub-function, and ``supports(routine, control)``,
which its own stage asks in Figure 30's order. ``UDSSVC_ARCH_0007`` records the reason:
whether a sub-function is supported is a property of the routine, so the lookup takes the
routine, which no other service's does, and the asymmetry is visible in the surface rather
than documented.

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

**7. ``uds_on_ip``'s client is entirely unimplemented — moot; the client sits on
``UdsTransport``.** It was filed as open across the stack because :doc:`client-surface` was
written to sit on that client, so this crate's client half could not run until it did.
Neither holds now. The client drives ``uds_session``'s client role over ``UdsTransport``
itself (``UDSSVC_ARCH_0029``), as the server does, and runs end to end over a scripted
transport. A ``DoIP`` client transport is the binding's to supply, not a dependency of this
crate.
