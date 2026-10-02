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

      B -> P
      P -> D
      D -> SF
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
           :decode with uds_protocol;
           if (decodes?) then (no)
             :0x13; <<negative>>
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
                 handler's own choosing.
                 The standard fixes the
                 order within a service
                 too — UDSSVC_ARCH_0042
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

   Rationale: 0x13 is a clause 8.7 outcome, but message length and format are properties of the
   encoding, which is ``uds_protocol``'s. So ``uds_protocol`` *detects* and this crate
   *maps*: neither re-derives the other's work, and there is exactly one place that knows
   the wire format. The request bytes are decoded with ``uds_protocol`` once Figure 5's
   support and session checks have passed (``UDSSVC_ARCH_0006``): decoding is the
   service-specific check, where Figure 5 places length and format. A decode failure
   settles the request with ``incorrectMessageLengthOrInvalidFormat`` (0x13). A request
   that decodes to ``uds_protocol``'s unmodelled-service variant settles with
   ``serviceNotSupported`` (0x11), not with 0x13.

   The unmodelled-service carve-out is the part that is easy to get wrong. ``uds_protocol``
   represents a service identifier it does not model as a variant carrying the raw service
   byte and payload, so that re-encoding is lossless for pass-through callers. Reaching
   that variant is not a format error — the request may be perfectly well formed — it means
   the server has no implementation for that service identifier, which is 0x11. Mapping it
   to 0x13 would report a malformed request to a client that sent a valid one.

   A service identifier that names no service at all never reaches the decode: the support
   check reads the service byte alone and settles it 0x11 first, as ISO 14229-1:2020
   8.7.5's pseudo-code does — its outer ``SWITCH`` on the service identifier falls to
   ``DEFAULT: responseCode = SNS`` before any ``message_length`` test. So a malformed
   request for a service this server lacks is 0x11, and one for a service refused in the
   active session is 0x7F; 0x13 is produced only for a service that passed both. Because
   ``uds_protocol`` validates each service's own payload during decode, Figure 6's
   minimum-length check is settled by that same decode. Figure 6's sub-function check
   reads the sub-function byte alone and runs before the decode, so a supported
   sub-function's exact length is tested only after it — see ``UDSSVC_ARCH_0007``.

   **A request with no service identifier is complete without a response.** ISO
   14229-1:2020 8.7.5's pseudo-code begins ``SWITCH (A_PDU.A_Data.A_PCI.SI)``: an A_PDU
   with no service identifier has no arm, so the standard does not model the case, and a
   negative response's ``SIDRQ`` would have nothing to echo. The pipeline's first check is
   therefore the empty request, which settles as silence; the driver still reports its
   completion to the session layer (``UDSS_LLR_0074``), because the request's reception
   stopped ``tS3_Server`` under ``UDSS_LLR_0087`` and only the completion report restarts
   it. This is a declared reading, recorded here with its citation.

Mandatory preconditions
-----------------------

.. arch:: The mandatory precondition stage follows Figure 5's order
   :id: UDSSVC_ARCH_0006
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0013; UDSSVC_ARCH_0015; UDSSVC_ARCH_0034
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2 Figure 5; ISO 14229-1:2020 10.2 Table 23
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
        - Table 23 where it decides; declared per service otherwise — see below

   Figure 5 places three optional checks and two manufacturer/supplier-specific hooks
   around this sequence; they are ``UDSSVC_ARCH_0011``.

   **The implemented order is Figure 5's, literally:** the empty request
   (``UDSSVC_ARCH_0005``'s declared reading), then check 1, then check 3, then — for a
   service with a SubFunction parameter other than 0x31 — Figure 6's sub-function checks
   (``UDSSVC_ARCH_0007``: supported ever, 0x12, then supported in the active session,
   0x7E), then the decode that settles 0x13 (``UDSSVC_ARCH_0005``). Nothing is decoded
   until the service is known to be supported and allowed in the active session, so 0x13
   never pre-empts 0x11 or 0x7F, and a trailing byte never pre-empts 0x12 or 0x7E.
   Check 2, authentication (0x34), and the security precondition (0x33) are not
   evaluated yet: authentication is architecture open question 4, and security joins with
   security state.

   **Check 3 is partly fixed by the standard, and this crate answers the fixed part.** Clause
   10.2's Table 23 states, for each service it lists, whether it is available in the
   ``defaultSession``. Twelve rows carry an unqualified **"not applicable"**, and those twelve
   are not a policy an integrator chooses:

   ``SecurityAccess`` (0x27), ``CommunicationControl`` (0x28), ``SecuredDataTransmission``
   (0x84), ``ControlDTCSetting`` (0x85), ``LinkControl`` (0x87),
   ``ReadDataByPeriodicIdentifier`` (0x2A), ``InputOutputControlByIdentifier`` (0x2F),
   ``RequestDownload`` (0x34), ``RequestUpload`` (0x35), ``TransferData`` (0x36),
   ``RequestTransferExit`` (0x37), ``RequestFileTransfer`` (0x38).

   Five further rows are footnoted and are genuinely the application's, because the footnote
   makes them conditional on something only the application knows:

   .. list-table::
      :header-rows: 1
      :widths: 42 58

      * - Row
        - Footnote
      * - ``ResponseOnEvent`` (0x86)
        - a — implementation specific whether it is also allowed during the defaultSession
      * - ``ReadDataByIdentifier`` (0x22), ``ReadScalingDataByIdentifier`` (0x24),
          ``WriteDataByIdentifier`` (0x2E)
        - b — secured dataIdentifiers require ``SecurityAccess``, hence a non-default session
      * - ``ReadMemoryByAddress`` (0x23), ``WriteMemoryByAddress`` (0x3D)
        - c — secured memory areas, likewise
      * - ``DynamicallyDefineDataIdentifier`` (0x2C)
        - d — may be defined dynamically in the default *and* non-default session
      * - ``RoutineControl`` (0x31)
        - e — secured routines require ``SecurityAccess``; a routine the client must stop
          actively also requires a non-default session

   So the check is the conjunction of Table 23's answer, where Table 23 gives one, and the
   application's declaration otherwise. ``UDSSVC_ARCH_0013``'s assembly list already names
   every supported service, so the fixed half needs no new input from anyone.

   This follows ``UDSSVC_ARCH_0034`` rather than preference: the standard fixes the twelve, so
   the twelve are implemented here; the standard defers the five, so the five are delegated. A
   server answering a ``RequestDownload`` in the ``defaultSession`` is not exercising a choice,
   it is failing a check nobody asked it to make. An earlier version of this element made the
   whole check application-declared.

   **What it costs**, stated because it is the first place this crate overrides an integrator
   rather than merely constraining one: a vehicle programme whose own session matrix disagrees
   with Table 23 on one of the twelve will find this crate refusing the request. Table 23 is
   normative and the refusal is correct, but whether an escape hatch is owed — and if so
   whether it is per service or whole-table — is not settled here. Note also that four of the
   twelve (0x2A, 0x2F, 0x87, 0x84) have no ``uds_protocol`` message type, so a third of the
   rule is unreachable for the reason ``UDSSVC_ARCH_0038`` records at the end.

   Two orderings here are worth stating explicitly, because both are counter-intuitive and
   both are observable. Figure 5 is an image in the markdown conversion of the standard and
   was read from the PDF on 2026-09-16; both orderings are confirmed there verbatim:

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
        - A request with no SubFunction byte is settled by the decode, which runs after
          row 2; see ``UDSSVC_ARCH_0005``
      * - 2
        - SubFunction supported ever for this service identifier?
        - 0x12
        - Read from the SubFunction byte, ``suppressPosRspMsgIndicationBit`` stripped,
          before any exact-length test: a trailing byte after an unsupported
          SubFunction is 0x12, not 0x13
      * - 3
        - Authentication check OK?
        - 0x34
        - Figure 6 repeats it at sub-function granularity. Not evaluated:
          authentication is architecture open question 4
      * - 4
        - SubFunction supported in the active session?
        - 0x7E
        - Asked only after row 2 accepted, so 0x7E is sent only for a SubFunction
          "known to be supported in another session" (Annex A); read from the same
          stripped byte, before any exact-length test

   The service's exact-length check is its service-specific check, after this stage:
   ISO 14229-1:2020 8.7.5's pseudo-code tests ``message_length`` only inside a supported
   sub-function's arm, and its ``DEFAULT: responseCode = SFNS`` precedes every such test.
   Row 3 is not evaluated yet: authentication is architecture open question 4, so it is
   unconditionally true, as Figure 5's authentication check is.

   **Rows 2 and 4 are each service trait's pair.** Whether a SubFunction is supported
   ever, and whether from the active session, are deployment facts — which sessions a
   server offers, and which it can be entered from — so the application states them, in
   the service's own typed SubFunction, and the pipeline decides the code. Annex A says
   0x7E "shall be supported by each diagnostic service with a SubFunction parameter",
   so every such trait carries both lookups once it has a stage:

   .. list-table::
      :header-rows: 1
      :widths: 26 37 37

      * - Service
        - Row 2 (0x12)
        - Row 4 (0x7E)
      * - ``DiagnosticSessionControl``
        - ``supports(session)``
        - ``supported_from(session, active)``, with no default body: an application that
          restricts no transition says so by returning ``true``
      * - ``TesterPresent``
        - fixed: only ``zeroSubFunction``
        - fixed: always. ``zeroSubFunction`` is the service's only SubFunction (clause
          10.7) and Table 23 allows the service in every session, so there is nothing
          deployment-specific to ask and the trait carries no lookup
      * - ``EcuReset``, ``CommunicationControl``, ``ControlDTCSetting``,
          ``SecurityAccess`` and the other SubFunction-bearing services
        - gains a ``supports``-shaped lookup when its stage lands
        - gains a ``supported_from``-shaped lookup, in its own SubFunction type, with it

   ``uds_server!`` hands ``pipeline::begin`` one closure per row; each routes the
   question to the listed service's trait and decides nothing (``UDSSVC_ARCH_0013``).
   ``pipeline::begin`` asks row 4 only once row 2 has accepted, and a SubFunction value
   that names nothing the service defines — a reserved session byte — is row 2's
   0x12 and never 0x7E.

   Figure 6 then places a sub-function security check (0x33) and a request-sequence check
   (0x24) in its optional column; both are ``UDSSVC_ARCH_0011``.

   The 0x31 exclusion is drawn from Figure 5's decision node, which reads "service with
   SubFunction, but not SID 0x31". It is not an editorial slip and it must be honoured:
   ``RoutineControl``'s sub-function is only meaningful together with the routine
   identifier that follows it, so whether a given sub-function is "supported" cannot be
   decided without the identifier. Sending 0x12 for a routine control type that is valid
   for some routines and not others would be wrong, so the decision is deferred to the
   service-specific check where both parameters are in hand.

   **The trait shape this implies is settled, and it settles open question 5.**
   ``RoutineControl`` carries three methods — ``start``, ``stop`` and ``results``, one per
   sub-function of clause 13.2 — rather than one method taking the sub-function as a
   parameter. The question the set had been carrying was whether Figure 5's exclusion should
   be expressed in the type or merely documented on a trait shaped like every other
   service's. Three methods is the answer, for a reason that outranks taste: this is the
   only service whose *handler* decides ``subFunctionNotSupported`` (0x12), because it is
   the only one the centralised sub-function stage does not run for. A single method taking
   a sub-function byte would leave that asymmetry invisible — the signature would be
   indistinguishable from every service whose 0x12 is settled before the handler is reached,
   and an implementor would have no prompt to answer it. Split into three, a routine that
   cannot be stopped returns 0x12 from ``stop`` and from nowhere else, and the exclusion is
   a property a reader can see in the surface rather than one they must be told about.

   The cost is stated rather than discovered: adding a sub-function to clause 13 would add a
   method to the trait, which is a breaking change where a parameter would not have been.
   Clause 13.2's three have been stable across editions, and a service whose handler owns
   the 0x12 decision has to gain a case either way.

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

   The same clause gives the server one limit it may impose: "the server may limit the number
   of dataIdentifiers that can be simultaneously requested as agreed upon by the vehicle
   manufacturer and system supplier". That is a per-server constant of the kind
   ``UDSSVC_ARCH_0033`` already collects on a service trait, and nothing in the set declares
   it yet. Exceeding it is a ``requestOutOfRange`` (0x31) by the same path as an unsupported
   identifier.

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
   :depends_on: UDSSVC_ARCH_0015; UDSSVC_ARCH_0032
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.3 Table 5; ISO 14229-1:2020 8.7.4.3 Table 7; ISO 14229-1:2020 8.7.5; ISO 14229-1:2020 A.1
   :tags: dispatch; suppression

   The final stage takes the settled response code, the addressing mode, the
   ``suppressPosRspMsgIndicationBit`` and whether this dispatch offered a response-pending,
   and decides between transmitting and silence.

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
      in rule 1. Annex A.1 states both limbs in one sentence, which is worth quoting because
      the tables state only the first:

         When this NRC is used, the server shall always send a final response (positive or
         negative) independent of the suppressPosRspMsgIndicationBit value **or the suppress
         requirement for responses with NRCs SNS, SFNS, SNSIAS, SFNSIAS and ROOR on
         functionally addressed requests.**

      Tables 4 and 5 carry only "independent of the suppressPosRspMsgIndicationBit value", so
      a reading taken from them alone gets rule 3 half right — which is the failure mode this
      rule exists to prevent.

   .. uml::
      :align: center
      :caption: The gate. The response-pending override is drawn first because it
                short-circuits both suppressions.

      @startuml
      start
      :settled code, addressing mode,
      suppress bit, response-pending offered;

      if (response-pending offered for this request?) then (yes)
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

   Rule 3 reads a fact the pipeline owns rather than an input it is given.
   ``UDSSVC_ARCH_0032`` makes this crate the originator of a response-pending, so the gate
   consults whether *this dispatch* offered one across ``UDSSVC_ARCH_0031``'s seam.

   **The gate reads "offered and accepted", where clause 8.7.5 writes "sent", and the
   divergence is deliberate.** A submission the session layer refuses is not a send, and the
   refusal is reported synchronously, so ``UDSSVC_ARCH_0031``'s ``offer`` returns it and the
   gate does not fire on it. This matters because refusal does **not** imply that an earlier
   response-pending got out: the spacing and unconfirmed-predecessor limbs
   (``UDSS_LLR_0119``, ``UDSS_LLR_0118``) do presuppose one, but ``UDSS_LLR_0061`` and
   ``UDSS_LLR_0062`` refuse a submission for a duplicated or exhausted transmission
   association, which can catch the *first* offer for a request with nothing yet sent.

   What remains is the accepted submission whose transmission later fails.
   ``UDSS_LLR_0110`` has such a transmission never reach the data link, so the client saw no
   response-pending and is still waiting out ``tP2``, where an unsuppressed final response is
   harmless.

   The asymmetry settles the rest. Treating an accepted offer as sent can cost one message a
   waiting client accepts; treating a sent response-pending as unsent produces silence where
   the standard requires a final response — the failure that needs both a slow handler and
   functional addressing to appear, and so will not show up in ordinary testing. Closing the
   gap the other way, by tracking the ``T_Data.conf``, would oblige the driver to report
   confirmations on this crate's behalf, which is a correctness obligation it has no way to
   discover it is failing.

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

.. arch:: This crate decides and composes the response-pending
   :id: UDSSVC_ARCH_0032
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0010; UDSSVC_ARCH_0013; UDSSVC_ARCH_0033
   :status: draft
   :origin: derived
   :tags: dispatch; nrc; response-pending

   When a handler has not settled the request and a response-pending becomes due, this
   crate decides whether one is admissible and, if it is, composes it. The driver
   transmits; it does not decide, and it does not assemble the bytes.

   Admissibility is the conjunction of two facts, both resolved before the handler runs:
   the service is implemented by this server (``UDSSVC_ARCH_0013``), and the service's own
   ``MAY_RESPOND_PENDING`` is true (``UDSSVC_ARCH_0033``).

   Rationale: ISO 14229-2 states the prohibition and states it in terms only this layer can
   evaluate. REQ 5.6 requires that services the server does not support use a
   ``tP4_Server_Max`` equal to ``tP2_Server_Max`` and that a response-pending "shall not be
   allowed" for them; REQ 5.4 gives that equality the same meaning "for the service in
   progress". *Does this server support this service* is the predicate, and it is the same
   fact, already computed, that produces ``serviceNotSupported`` (0x11) in
   ``UDSSVC_ARCH_0006``. A session layer cannot evaluate it — ``uds_session`` is forbidden
   to inspect message data and holds its protocol parameters per instance rather than per
   service — and a byte-level driver has no notion of which services a server implements.

   **What this crate implements is the predicate, not the requirement.** It holds no
   ``tP4_Server_Max``, runs no timer, and knows no timing parameter; ``tP4_Server_Max`` is
   how ISO 14229-2 *expresses* a per-service permission, and ``UDSSVC_ARCH_0033`` is where
   that permission is declared, by the application.

   REQ 5.6's own case is discharged without a check at all. Figure 5's first mandatory
   condition settles an unsupported service at 0x11 in the precondition stage, so no handler
   runs and ``UDSSVC_ARCH_0031``'s ``due`` never fires for one; and a service the server does
   not implement has no trait impl from which ``MAY_RESPOND_PENDING`` could be read. Clause
   8.7's own ordering satisfies it twice over.

   The bytes are this crate's for the reason ``UDSSVC_ARCH_0010`` already gives: a
   response-pending is a negative response — ``0x7F``, the echoed service identifier,
   ``0x78`` — and ISO 14229-1:2020 A.1 gives it no payload. Composing it below this layer
   would put a negative response in a crate that "does not know what a service does", and
   would need the echoed service identifier that only the decoded pipeline holds.

   What this element does **not** claim is the timing. When a response-pending becomes due,
   how often it may repeat, and whether one was accepted are ISO 14229-2's and reach this
   crate only as ``UDSSVC_ARCH_0031``'s ``due``. This crate answers *whether* and *what*;
   the session layer and the driver answer *when*.

   Two consequences recorded elsewhere. The suppression override of ``UDSSVC_ARCH_0009``
   rule 3 becomes a fact this crate owns rather than an input, which is why
   ``UDSSVC_ARCH_0015`` carries no response-pending field. And ``uds_session`` needs no new
   output reporting that a 0x78 has been sent; an earlier draft asked for one.

The service-specific check
--------------------------

.. arch:: Each service's own negative response order is normative and this crate's
   :id: UDSSVC_ARCH_0042
   :part_of: UDSSVC_ARCH_0004
   :depends_on: UDSSVC_ARCH_0012; UDSSVC_ARCH_0034
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.2 Figure 5; ISO 14229-1:2020 8.7.3.1 Figure 6; ISO 14229-1:2020 Figures 11, 20-23, 26-35
   :tags: dispatch; nrc; traits

   Figure 5 ends at a box reading "Specific SID CHECK" and Figure 6 at one reading "Specific
   SID checks". That box is not the end of the standard's ordering — it is a handover to
   fifteen further normative figures, one per service, each fixing the order in which that
   service's own negative response codes are evaluated. Each is introduced by the same
   sentence in its service's clause: *"The evaluation sequence is documented in Figure N."*

   The figures are 11, 20, 21, 22, 23, 26, 27, 28, 29, 30, 31, 32, 33, 34 and 35.

   **The ordering is normative, so it is this crate's**, by ``UDSSVC_ARCH_0034`` and by
   ``UDSSVC_ARCH_0004``'s own argument one level down: a server that reorders these checks
   produces a *different* negative response code for the same request, which is
   correct-looking behaviour that fails conformance. That is as true of Figure 33's order
   within ``TransferData`` as of Figure 5's order across services. Nothing distinguishes the
   two cases except that one is drawn in clause 8.7 and the other in clause 15.

   **It is not built, and this element exists so that is a recorded gap rather than an
   inference from silence** — ``UDSSVC_ARCH_0001`` holds that a clause with no seam and no
   implementation is unbuilt, not out of scope. Until it is built, ``UDSSVC_ARCH_0004``'s
   pipeline ends at the handler and a handler returns "a code of its own choosing", which
   means the per-service order is the application's to get right. That is the arrangement this
   crate exists to remove, and it is temporary.

   What is not yet designed is the seam. A service trait would have to express its evaluation
   order in a form the dispatcher can apply, and the candidates differ sharply in cost: a
   declared sequence of predicates per trait, a fixed set of typed pre-handler hooks per
   service, or generated code per figure. Choosing between them wants more than one figure
   read in full, which is a pass of its own.

   Rationale for recording it now rather than then: four services are in the first slice, and
   their figures are small enough to carry in the handler contract meanwhile. The cost of not
   recording it is that the gap reads as a decision — the set would appear to have concluded
   that per-service ordering is the application's, which it has not.

   An earlier version of this set had no element here at all, and none of ``not-owned``,
   ``UDSSVC_ARCH_0001``'s in-scope list or :doc:`open-questions` mentioned these figures.

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
        - manufacturer
        - Manufacturer-specific failure detected
        - Figure 5, after the busy check and before the service-identifier check
      * - any
        - supplier
        - Supplier-specific failure detected
        - Figure 5, after 0x39 and before the SubFunction branch
      * - any
        - manufacturer / supplier
        - Manufacturer- or supplier-specific check
        - Figure 6, after 0x24 and before the service-specific check

   These are attachment points rather than implementations because none of them can be
   decided here. Whether the server is busy is a property of the application's own
   scheduling; what constitutes a satisfied security level is manufacturer-defined; and
   ISO 14229-1 explicitly reserves the manufacturer/supplier column.

   The *position* is the part this crate owns and the part worth centralising. A caller
   that implements its own busy check in the wrong place answers 0x11 where the standard
   requires 0x21, and no amount of care inside the check itself fixes that.

   The two hooks in Figure 5 are distinct and differently placed — one **manufacturer**, one
   **supplier** — and an earlier version of this element gave them a single row reading "Figure
   5 has two such hooks". They are separated above because a caller attaching a check to the
   wrong one of them gets the wrong precedence against the service-identifier and security
   checks between them.

   **A transcription trap in the source, recorded so it is not re-derived.** The two figures
   draw these hooks with *opposite polarity*: Figure 5's nodes ask "failure detected?" and take
   the NRC exit on **YES**, while Figure 6's asks a check question and takes the NRC exit on
   **NO**. The behaviour is the same; only the drawing differs. Whoever authors the requirement
   should read the figure rather than copy this table.

   Figure 5's busy check also carries a Key note narrowing it: the request "cannot be accepted
   because another diagnostic task is already requested and in progress **by a different
   client**". That is narrower than "the server is busy", and it is a per-channel question
   under ``UDSSVC_ARCH_0035`` rather than a global one.

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

Two details of that wording matter to where the line falls, and both were missing from an
earlier version of this passage.

**The second exception is conditional.** The clause opens "**If a server supports services in
the range of 0x00 to 0x0F** receives diagnostic requests in the range of 0x00 to 0x0F …". The
whole rule is predicated on the server implementing something in that range — which is a fact
``UDSSVC_ARCH_0013``'s assembly list holds and a byte-level driver cannot see. That
strengthens the proposed split rather than complicating it: classification needs what this
crate knows.

**Occupancy ends on silence too.** The instance is held "until the request message is
processed (with final response sent **or application call without response**)", so an
``Outcome::Suppress`` releases it exactly as a transmitted response does. A driver that
released the instance only on a transmission would deadlock on the silence clause 8.7
requires.

Occupying a resource, bypassing it, and aborting an in-flight service are all properties
of the driver loop that calls this crate, not of a single dispatch. What belongs here is
at most the classification — which requests qualify for each exception — and that
allocation is unsettled. See :doc:`open-questions`.
