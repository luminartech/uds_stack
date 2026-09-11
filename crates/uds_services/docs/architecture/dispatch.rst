The dispatch pipeline
=====================

Clause 8.7 as a sequence of stages: what each one decides, which negative response code it
can produce, and where a caller's own checks attach.

The order is not an implementation detail. Figure 5 and Figure 6 fix it, and a server that
reorders the checks produces a *different* negative response code for the same request —
correct-looking behaviour that fails conformance.

Overview
--------

.. arch:: Dispatch is an ordered pipeline of validation stages
   :id: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0002
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2 Figure 5; ISO 14229-1:2020 8.7.3.1 Figure 6; ISO 14229-1:2020 8.7.5
   :tags: dispatch

   A request is processed by stages in a fixed order. Each stage either passes the request
   to the next or settles it with a response code; the last stage decides whether the
   settled outcome is transmitted at all.

   .. uml::
      :align: center

      @startuml
      left to right direction
      rectangle "bytes" as B
      rectangle "decode" as D
      rectangle "preconditions" as P
      rectangle "sub-function" as SF
      rectangle "data parameters" as DP
      rectangle "handler" as H
      rectangle "suppression gate" as G
      rectangle "Responded\nor Suppress" as O

      B -> D
      D -> P
      P -> SF
      SF -> DP
      DP -> H
      D -[#C0392B]-> G : settles
      P -[#C0392B]-> G : settles
      SF -[#C0392B]-> G : settles
      DP -[#C0392B]-> G : settles
      H -> G
      G -> O
      @enduml

   Two properties follow from the shape and are relied on throughout:

   * A stage may only settle the request; it may not revisit an earlier decision. The
     first stage to fail determines the code, which is what makes the *order* observable
     from outside.
   * The pipeline is asynchronous, and the only stage that actually awaits is the handler
     (``UDSSVC_ARCH_0016``). Every validation stage is a pure function of the request and
     the context; none of them can block, and none of them needs to.
   * Suppression is applied last, to the settled code, and never earlier. A stage does not
     know whether its own negative response will be transmitted — see
     ``UDSSVC_ARCH_0009``.

The cascade in full
-------------------

Every check, in order, with the code it produces. The nesting is the point: the *first*
check to fail settles the request, which is why reordering the checks changes the answer
rather than merely the code path.

.. uml::
   :align: center
   :caption: The mandatory path. Optional and manufacturer-specific checks
             (``UDSSVC_ARCH_0011``) are omitted; each attaches at a stated position.

   @startuml
   start
   :decode with uds_protocol;
   if (decodes?) then (no)
     :0x13; <<negative>>
   else (yes)
     if (service identifier modelled?) then (no)
       :0x11; <<negative>>
     else (yes)
       if (service implemented?) then (no)
         :0x11; <<negative>>
       else (yes)
         if (authenticated?) then (no)
           :0x34; <<negative>>
         else (yes)
           if (supported in active session?) then (no)
             :0x7F; <<negative>>
           else (yes)
             if (has SubFunction, and not SID 0x31?) then (yes)
               if (SubFunction supported?) then (no)
                 :0x12; <<negative>>
               else (yes)
                 if (SubFunction in active session?) then (no)
                   :0x7E; <<negative>>
                 else (yes)
                   :SubFunction accepted;
                 endif
               endif
             else (no)
               :no SubFunction, or SID 0x31:
               Table 6 or Table 7 applies;
             endif
             if (any data parameter supported?) then (none)
               :0x31; <<negative>>
             else (at least 1)
               :run the handler;
               note right
                 Ok, or a code of the
                 handler's own choosing
               end note
             endif
           endif
         endif
       endif
     endif
   endif
   :suppression gate;
   stop
   @enduml

Two things this diagram makes visible that the tables below do not. **Authentication sits
above the session check**, so a request in the wrong session from an unauthenticated
client is answered 0x34 and never reaches the 0x7F branch. And **every exit converges on
the suppression gate** — including the negative ones, and including silence. No stage
decides whether its own answer is transmitted.

Decode
------

.. arch:: Decoding is delegated, and its failures map to 0x13
   :id: UDSSVC_ARCH_0005
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0002
   :status: draft
   :origin: derived
   :tags: dispatch; uds_protocol

   The first stage decodes the request bytes with ``uds_protocol``. A decode failure
   settles the request with ``incorrectMessageLengthOrInvalidFormat`` (0x13). A request
   that decodes to ``uds_protocol``'s unmodelled-service variant settles with
   ``serviceNotSupported`` (0x11), not with 0x13.

   Rationale: 0x13 is a clause 8.7 outcome, but message length and format are properties
   of the encoding, which is ``uds_protocol``'s. So ``uds_protocol`` *detects* and this
   crate *maps*: neither re-derives the other's work, and there is exactly one place that
   knows the wire format.

   The unmodelled-service carve-out is the part that is easy to get wrong. ``uds_protocol``
   represents a service identifier it does not model as a variant carrying the raw service
   byte and payload, so that re-encoding is lossless for pass-through callers. Reaching
   that variant is not a format error — the request may be perfectly well formed — it means
   the server has no implementation for that service identifier, which is 0x11. Mapping it
   to 0x13 would report a malformed request to a client that sent a valid one.

   A consequence worth naming: because ``uds_protocol`` validates each service's own
   payload during decode, several of the length checks Figure 6 places *inside* the
   pipeline are already settled before the pipeline runs. This changes where 0x13 is
   produced, not whether it is — see ``UDSSVC_ARCH_0007``.

Mandatory preconditions
-----------------------

.. arch:: The mandatory precondition stage follows Figure 5's order
   :id: UDSSVC_ARCH_0006
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0013; UDSSVC_ARCH_0015
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2 Figure 5
   :tags: dispatch; nrc

   The stage evaluates Figure 5's mandatory checks in the standard's order:

   .. list-table::
      :header-rows: 1
      :widths: 8 34 12 46

      * - #
        - Check
        - Code
        - Notes
      * - 1
        - Service identifier supported?
        - 0x11
        - Determined by which service traits the server implements
          (``UDSSVC_ARCH_0013``)
      * - 2
        - Authentication check OK?
        - 0x34
        - Mandatory in Figure 5, and it precedes the session check
      * - 3
        - Service identifier supported in active session?
        - 0x7F
        - Declared per service against ``Ctx::active_session``

   Figure 5 places three optional checks and two manufacturer/supplier-specific hooks
   around this sequence; they are ``UDSSVC_ARCH_0011``.

   Two orderings here are worth stating explicitly, because both are counter-intuitive and
   both are observable:

   * **Authentication precedes the session check.** A request for a supported service in
     the wrong session, from an unauthenticated client, is answered 0x34 and not 0x7F.
   * **"Supported" and "supported in the active session" are separate decisions**
     producing separate codes (0x11 versus 0x7F, and 0x12 versus 0x7E on the sub-function
     path). A server that folds them together answers 0x11 where the standard requires
     0x7F, and the difference tells a client whether to change session or give up.

.. arch:: The sub-function branch excludes service identifier 0x31
   :id: UDSSVC_ARCH_0007
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0005
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2 Figure 5; ISO 14229-1:2020 8.7.3.1 Figure 6
   :tags: dispatch; nrc

   After the mandatory preconditions, a request for a service that carries a SubFunction
   parameter enters the sub-function stage, **except** for service identifier 0x31
   (``RoutineControl``), which proceeds directly to the service-specific check.

   Figure 6's stage, in its order:

   .. list-table::
      :header-rows: 1
      :widths: 8 40 12 40

      * - #
        - Check
        - Code
        - Notes
      * - 1
        - Minimum length (service identifier + SubFunction)
        - 0x13
        - Largely settled during decode; see ``UDSSVC_ARCH_0005``
      * - 2
        - SubFunction supported ever for this service identifier?
        - 0x12
        -
      * - 3
        - Authentication check OK?
        - 0x34
        - Figure 6 repeats it at sub-function granularity
      * - 4
        - SubFunction supported in the active session?
        - 0x7E
        -

   Figure 6 then places a sub-function security check (0x33) and a request-sequence check
   (0x24) in its optional column; both are ``UDSSVC_ARCH_0011``.

   The 0x31 exclusion is drawn from Figure 5's decision node, which reads "service with
   SubFunction, but not SID 0x31". It is not an editorial slip and it must be honoured:
   ``RoutineControl``'s sub-function is only meaningful together with the routine
   identifier that follows it, so whether a given sub-function is "supported" cannot be
   decided without the identifier. Sending 0x12 for a routine control type that is valid
   for some routines and not others would be wrong, so the decision is deferred to the
   service-specific check where both parameters are in hand.

Data parameters
---------------

.. arch:: Partial data-parameter support yields a positive response
   :id: UDSSVC_ARCH_0008
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0014
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.1; ISO 14229-1:2020 8.7.3.2 Table 4; ISO 14229-1:2020 8.7.4.2 Table 6; ISO 14229-1:2020 11.2.1
   :tags: dispatch; nrc

   For a request carrying data parameters, the stage classifies the server's support for
   them as ALL, At least 1, or None, per clause 8.7.1:

   * **ALL** or **At least 1** supported: a positive response, containing the supported
     parameters only.
   * **None** supported: ``requestOutOfRange`` (0x31).

   This is a normative classification, not a policy choice. It means a
   ``ReadDataByIdentifier`` request naming three identifiers of which the server supports
   one is answered *positively*, with the one record — not with 0x31, and not with a
   negative response naming the unsupported identifiers. A server that rejects the whole
   request because part of it was unsupported is non-conformant.

   A second rule from the same clause, easy to miss and easy to violate by being helpful:
   clause 11.2.1 states that a request message may contain the same data identifier more
   than once, and that the server shall treat each as a separate parameter and respond
   with data for each as often as requested. **Duplicates are not de-duplicated.** A
   dispatcher that collected the requested identifiers into a set — the obvious way to
   write it — would answer a request naming an identifier twice with one record, and be
   non-conformant.

   The rule shapes the service trait signatures: a per-identifier handler must be callable
   once per requested identifier, and "unsupported identifier" must be distinguishable
   from "identifier supported but the read failed". The first contributes to the ALL / At
   least 1 / None classification; the second is a processing error that settles the whole
   request with its own code. See ``UDSSVC_ARCH_0014``.

Suppression
-----------

.. arch:: Suppression is decided last, from the code and the addressing mode
   :id: UDSSVC_ARCH_0009
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0015
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.3 Table 5; ISO 14229-1:2020 8.7.4.3 Table 7; ISO 14229-1:2020 8.7.5
   :tags: dispatch; suppression

   The final stage takes the settled response code, the addressing mode, the
   ``suppressPosRspMsgIndicationBit`` and whether a response-pending negative response has
   already been sent, and decides between transmitting and silence.

   Three rules, all from clause 8.7.5's pseudo-code and Tables 5 and 7:

   1. **Functional addressing suppresses five negative response codes.** On a functionally
      addressed request, ``serviceNotSupported`` (0x11),
      ``serviceNotSupportedInActiveSession`` (0x7F), ``SubFunctionNotSupported`` (0x12),
      ``SubFunctionNotSupportedInActiveSession`` (0x7E) and ``requestOutOfRange`` (0x31)
      produce **no response at all**. The same failures on a physically addressed request
      produce a negative response.

   2. **The suppress-positive-response bit suppresses positive responses only.** It is
      ignored for every negative response, on both addressing modes.

   3. **A sent response-pending overrides both suppressions.** Once
      ``requestCorrectlyReceivedResponsePending`` (0x78) has been sent, a final response
      shall be sent, independent of the bit *and* of the functional-addressing suppression
      in rule 1.

   .. uml::
      :align: center
      :caption: The gate. The response-pending override is drawn first because it
                short-circuits both suppressions.

      @startuml
      start
      :settled code, addressing mode,
      suppress bit, response-pending state;

      if (response-pending already sent?) then (yes)
        :transmit; <<positive>>
        note right
          Clause 8.7.5: a final response
          shall be given regardless of
          either suppression.
        end note
        stop
      else (no)
      endif

      if (settled code is negative?) then (yes)
        if (functionally addressed
        AND code is 0x11, 0x12, 0x31, 0x7E or 0x7F?) then (yes)
          :Suppress; <<silent>>
          stop
        else (no)
          :transmit negative response; <<negative>>
          note right
            The suppress bit is ignored
            for any negative response.
          end note
          stop
        endif
      else (no)
        if (suppress bit set?) then (yes)
          :Suppress; <<silent>>
          stop
        else (no)
          :transmit positive response; <<positive>>
          stop
        endif
      endif
      @enduml

   Rule 1 is the rule most likely to be implemented wrongly, and the reasoning behind it
   is worth carrying: a functionally addressed request reaches every server on the bus, so
   a bus-wide chorus of "I do not support that" would be worse than useless. Note what it
   does *not* cover — a processing error on a request whose service, sub-function and at
   least one data parameter were all supported is still answered negatively, because that
   answer is specific to this server rather than a report of non-participation.

   Rule 3 is why the pipeline cannot be a pure function of the request and the server
   alone. Whether 0x78 has gone out is session-layer state that this crate does not own
   and cannot observe, so it is an input: ``UDSSVC_ARCH_0015``.

   Rule 2 has a corollary for services without a SubFunction parameter: they have no
   ``suppressPosRspMsgIndicationBit`` at all, so a positive response to one is never
   suppressed.

.. arch:: A negative response is written as three bytes, not returned
   :id: UDSSVC_ARCH_0010
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0017
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.5; ISO 14229-1:2020 8.6
   :tags: dispatch; nrc

   A transmitted negative response is the negative response service identifier 0x7F, the
   service identifier of the request being answered, and the negative response code. It is
   written into the caller's sink by the same path as a positive response.

   The request's *original* service identifier is echoed, including for a request whose
   service this server does not model — which is why ``UDSSVC_ARCH_0005`` keeps the raw
   service byte rather than discarding it once it has decided on 0x11. A negative response
   naming a different service than the client asked about cannot be matched to the request
   that caused it.

Extension points
----------------

.. arch:: Optional and manufacturer-specific checks are caller-supplied
   :id: UDSSVC_ARCH_0011
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0015
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2; ISO 14229-1:2020 8.7.2 Figure 5; ISO 14229-1:2020 8.7.3.1 Figure 6
   :tags: dispatch; extension

   Clause 8.7.2 classifies validation steps as mandatory, optional, or
   manufacturer/supplier specific. This crate implements the mandatory steps and provides
   attachment points for the rest, at the positions Figures 5 and 6 place them:

   .. list-table::
      :header-rows: 1
      :widths: 14 18 30 38

      * - Code
        - Class
        - Check
        - Position
      * - 0x21
        - optional
        - Server busy
        - Figure 5, before the service-identifier check
      * - 0x33
        - optional
        - Security check for the service identifier
        - Figure 5, after the session check
      * - 0x38
        - optional
        - Secure data transmission required, message not secure
        - Figure 5, after 0x33
      * - 0x39
        - optional
        - Secure data transmission required, message secure
        - Figure 5, after 0x38
      * - 0x33
        - optional
        - Security check for the SubFunction
        - Figure 6, after the session check
      * - 0x24
        - optional
        - Request sequence respected for the SubFunction
        - Figure 6, after 0x33
      * - any
        - manufacturer / supplier
        - Manufacturer- and supplier-specific failures
        - Figure 5 has two such hooks, Figure 6 one

   These are attachment points rather than implementations because none of them can be
   decided here. Whether the server is busy is a property of the application's own
   scheduling; what constitutes a satisfied security level is manufacturer-defined; and
   ISO 14229-1 explicitly reserves the manufacturer/supplier column.

   The *position* is the part this crate owns and the part worth centralising. A caller
   that implements its own busy check in the wrong place answers 0x11 where the standard
   requires 0x21, and no amount of care inside the check itself fixes that.

   Clause 8.7.2 carries a note that, given the choices available across these figures, a
   specific negative response code is not guaranteed for every possible test-pattern
   sequence. This crate therefore fixes the mandatory order exactly and leaves the optional
   checks to the caller, rather than choosing a set of optional checks on the caller's
   behalf and presenting the result as the conformant one.

The stages, generated
---------------------

The same pipeline, drawn from the elements rather than by hand. Every node links to the
element it stands for.

.. needflow::
   :filter: id == "UDSSVC_ARCH_0004" or "UDSSVC_ARCH_0004" in part_of
   :link_types: depends_on, part_of
   :align: center

The edges leaving the group are the interesting part: the stages depend on the seams and on
the API surface, never the reverse. A stage that needed something from a *later* stage
would show up here as a cycle.

Concurrency
-----------

Clause 8.7.6 is a subclause of clause 8.7, but the behaviour it describes is not this
crate's. It states that a server has one diagnostic protocol instance, that any received
request occupies it until processing completes, and it carves out two exceptions: a
functionally addressed ``TesterPresent`` with the suppress bit set must bypass that
occupancy, and a request in the 0x00–0x0F service range must abort an active service
outside that range and start the default session, unless a programming session is active.

Occupying a resource, bypassing it, and aborting an in-flight service are all properties
of the driver loop that calls this crate, not of a single dispatch. What belongs here is
at most the classification — which requests qualify for each exception — and that
allocation is unsettled. See :doc:`open-questions`.
