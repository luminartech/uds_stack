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
   * - **When a response-pending is due, and how often it may repeat**
     - ``uds_session``
     - ISO 14229-2 timing; see below
   * - ``tP2_Server`` / ``tP2*_Server``
     - ``uds_session``
     - ISO 14229-2 timers
   * - ``tP4_Server``
     - this crate, but not as a timer
     - ISO 14229-2 types it a *performance requirement* on the application, not
       something to run
   * - Framing, connection setup, routing activation
     - the binding
     - ISO 14229-5 and ISO 13400-2; reached through ``UDSSVC_ARCH_0029``
   * - A_PDU framing, TCP handling, DoIP negative acknowledgements
     - ``uds_on_ip``
     - ISO 14229-5
   * - Occupying the diagnostic protocol instance
     - unsettled
     - Clause 8.7.6 describes a resource, not a dispatch — but the driver that would
       hold it is now this crate's. See :doc:`open-questions`
   * - Establishing and configuring the link
     - the consuming application
     - Not portable across transports; ``UDSSVC_ARCH_0003``

Response-pending: what moved, and what did not
----------------------------------------------

This entry was decided the other way in an earlier draft, and the correction is recorded
rather than quietly applied, because the reasoning that produced it was sound and is worth
knowing was superseded.

The original argument was that emitting a 0x78 cannot be this crate's job because "a
handler that is still working is by definition not returning, so it cannot also be watching
a clock". That was correct **for a synchronous handler**. ``UDSSVC_ARCH_0016`` made the
handler seam asynchronous, which removed the premise: dispatch is suspended at an await
while the handler works, and ``UDSSVC_ARCH_0031``'s seam is what lets it act in that window.
The conclusion outlived the premise by several drafts, having been re-homed onto
``uds_session`` where it looked settled.

It was not settled there. ``uds_session`` declines the job in its own requirement set:
``UDSS_LLR_0117`` reports the ``tP2_Server`` overrun and states that it "must not be read
as obliging the session layer to produce a response", and ``UDSS_LLR_0119`` expects the
request for a response-pending to *arrive from its caller*, which it may then refuse. With
``UDSSVC_ARCH_0040`` that caller is this crate, so the two ends of the argument have met.

``UDSSVC_ARCH_0032`` now places the decision and the bytes here, on the standard's own
grounds rather than on convenience: ISO 14229-2:2021 REQ 5.4 and REQ 5.6 make
admissibility a per-service question turning on whether the server supports the service,
which is a clause 8.7 fact this crate already computes.

**What remains not owned here is the timing**, and the division is clean:

.. list-table::
   :header-rows: 1
   :widths: 34 66

   * - This crate answers
     - The session layer and driver answer
   * - *Whether* a response-pending is admissible, and *what* bytes it is
     - *When* one is due, how often it may repeat, and whether one was accepted

.. uml::
   :align: center
   :caption: A handler that outruns ``tP2_Server``. The handler yielding is what lets the
             driver observe the overrun at all.

   @startuml
   autonumber "<b>[0]"
   hide footbox

   actor tester as T
   participant "uds_on_ip" as I
   box "uds_services" #F4F7FA
     participant "driver loop" as L
     participant "uds_session" as S
     participant "dispatch" as V
   end box
   participant "slow handler" as H

   L -> V : dispatch(bytes, sink)
   activate V
   V -> H : typed handler call
   activate H

   == tP2_Server expires while the handler yields ==

   L -> S : tick(now)
   S --> L : a reaction
   loop drain with Reaction::outputs()
     S --> L : ServerOutput::ResponseOverrun
   end
   L -> S : finish()
   L -> V : is a 0x78 admissible?
   V --> L : yes — service implemented\nand MAY_RESPOND_PENDING
   L -> S : s_data_req(now, ai, [7F, sid, 78],\nServerTx::ResponsePending)
   S --> L : a reaction
   loop drain with Reaction::outputs()
     S --> L : ServerOutput::Transmit { ai, data }
     L -> I : t_data_req(ai, data)
     I -> T : 0x78
   end
   L -> S : finish()
   S --> L : accepted, or refused

   H --> V : Ok, or a negative response code
   deactivate H
   V -> V : suppression gate,\nresponse-pending submitted
   V --> L : Responded::Yes
   deactivate V
   L -> S : s_data_req(now, ai, response bytes,\nServerTx::FinalResponse)
   loop drain with Reaction::outputs()
     S --> L : ServerOutput::Transmit { ai, data }
     L -> I : t_data_req(ai, data)
     I -> T : final response
     note right of T
       Sent even if functionally addressed
       and the code is one of the five
       normally silenced.
     end note
   end
   L -> S : finish()
   @enduml

The constraint this used to place on a binding's driver now falls on this crate's own loop:
dispatch must not block it, or the ``ResponseOverrun`` is never observed and the
response-pending it would have prompted is never sent. ``UDSSVC_ARCH_0016``'s asynchronous
handler seam is what makes that achievable — the handler yields at its await points and the
loop keeps draining.

An earlier version of this diagram had a binding driver calling a ``due()`` on this crate
and receiving an ``offer()`` back. Both were seams to a component that no longer exists; see
``UDSSVC_ARCH_0031``.

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
