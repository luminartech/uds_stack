What this crate does not own
============================

Explicit because these were argued and settled, and boundaries are the easiest thing in a
design to erode. Each entry has a plausible argument for living here; each was decided
against ``UDSSVC_ARCH_0001``.

.. list-table::
   :header-rows: 1
   :widths: 30 22 48

   * - Concern
     - Owner
     - Why not here
   * - Message encode and decode
     - ``uds_protocol``
     - It is a codec; this crate is policy over it
   * - ``tP_Client``, ``tS3``, session state
     - ``uds_session``
     - ISO 14229-2
   * - **Deciding to send 0x78**
     - ``uds_session``
     - See below
   * - ``tP2_Server`` / ``tP2*_Server``
     - ``uds_session``
     - ISO 14229-2 timers
   * - ``tP4_Server``
     - this crate, but not as a timer
     - ISO 14229-2 types it a *performance requirement* on the application, not
       something to run
   * - The byte seam
     - the binding
     - Transport-shaped; see ``UDSSVC_ARCH_0018``
   * - A_PDU framing, TCP handling, DoIP negative acknowledgements
     - ``uds_on_ip``
     - ISO 14229-5
   * - Occupying the diagnostic protocol instance
     - the binding's driver
     - Clause 8.7.6 describes a resource, not a dispatch
   * - Connection setup, routing activation
     - the binding
     - Not portable across transports; ``UDSSVC_ARCH_0003``

Response-pending is the subtle one
----------------------------------

Emitting ``requestCorrectlyReceivedResponsePending`` (0x78) before ``tP2_Server`` expires
cannot be this crate's job, and the reason is structural rather than a matter of taste: a
handler that is still working is by definition not returning, so it cannot also be
watching a clock — and the clock is an ISO 14229-2 timer, which belongs to ``uds_session``.

So ``uds_session`` owns the timer and decides, the binding transmits, and this crate is
simply still running.

.. uml::
   :align: center
   :caption: A handler that outruns ``tP2_Server``. The handler yielding is what lets
             the driver reach step 3 at all.

   @startuml
   autonumber "<b>[0]"
   hide footbox

   actor tester as T
   participant "binding driver" as I
   participant "uds_session" as S
   participant "uds_services" as V
   participant "slow handler" as H

   I -> V : handle(ctx, bytes, sink)
   activate V
   V -> H : typed handler call
   activate H

   == tP2_Server expires while the handler runs ==

   S -> I : send response-pending
   I -> T : 0x78
   note over V
     uds_services is mid-dispatch and
     unaware. It reads no clock and
     is not asked. The handler has
     yielded, so the driver runs.
   end note

   H --> V : Ok, or a negative response code
   deactivate H
   V -> V : suppression gate,\nwith response-pending seen
   V --> I : Responded
   deactivate V
   I -> T : final response
   note right of T
     Sent even if functionally addressed
     and the code is one of the five
     normally silenced.
   end note
   @enduml

What this crate needs is not the decision but its *consequence*: that a 0x78 has been sent
changes the suppression rules, which is why it is an input to ``UDSSVC_ARCH_0015`` and a
rule in ``UDSSVC_ARCH_0009``. Step 6 is the whole reason that field exists — without it,
the gate silences a response the standard requires.

One consequence constrains the **binding's** server driver rather than this crate, and is
recorded here so it is not rediscovered later: dispatch must not block the loop that
drains session actions, or the 0x78 that ``uds_session`` decided to send cannot be
transmitted while the handler it covers is still running. ``UDSSVC_ARCH_0016``'s
asynchronous handler seam is what keeps a slow handler from having to run somewhere the
driver can continue past: the handler yields at its await points, and the driver simply
continues.

This constraint is load-bearing for conformance and is not satisfiable by anything this
crate does.

What the layer below declines
-----------------------------

The table above is what this crate hands off. The traffic also runs the other way, and it
is worth naming because it is the clearest statement anywhere in the stack of why this
crate exists.

``uds_on_ip``'s client returns a completion whose result is ``Ok`` for a UDS negative
response — correctly, because the exchange completed — and its documentation says: *"A UDS
negative response is not an error: it is a response, and interpreting it belongs to a
higher layer."* Its handler seam says the same in the other direction: it hands over bytes
plus context and notes that it does not know what a service is, what a data identifier is,
or which negative response code applies.

So the binding declines, in writing, both halves of the job this crate does. There is no
layer between the two, which makes the delegation direct rather than aspirational, and it
is what ``UDSSVC_ARCH_0019`` records.

Requirements are authored from the standard
-------------------------------------------

One more thing this document does not own: the requirement set. Requirements are authored
from ISO 14229-1, not from this architecture. A requirement set fitted to what a prototype
happens to do agrees with the code by construction, and the gap-closing step that follows
then finds nothing — which is the failure mode the whole process exists to prevent.

Read this document as evidence about what is *buildable*, and as the record of decisions
whose reasoning would otherwise be lost. Where an architecture element cites a clause, that
citation is a pointer for the person authoring the requirement, not a substitute for
authoring it.
