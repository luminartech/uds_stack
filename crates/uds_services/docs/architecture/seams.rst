The seams
=========

What crosses each boundary of this crate, and who owns what on either side.

Four seams: the context supplied with a request, the outcome reported back, the sink a
response is written into, and the adapter onto a binding's byte interface.

.. uml::
   :align: center
   :caption: The four seams. Everything crossing the crate boundary crosses one of them.

   @startuml
   package "binding (uds_on_ip, uds_on_can)" {
     [server driver] as DRV
     [uds_session] as SESS
   }

   package "uds_services" {
     [dispatch pipeline] as PIPE
     [binding adapter] as AD
   }

   package "application" {
     [typed service handlers] as APPH
   }

   interface "request bytes" as BYTES
   interface "Ctx" as CTX
   interface "Outcome" as OUT
   interface "Write sink" as SINK

   DRV -down-> BYTES
   BYTES -down-> AD
   DRV -down-> CTX
   SESS -up-> DRV : session, security,\nresponse-pending
   CTX -down-> AD
   AD -down-> PIPE
   PIPE -up-> OUT
   OUT -up-> DRV
   PIPE -down-> APPH : typed call
   APPH -up-> SINK
   SINK -up-> DRV : bytes the driver owns
   @enduml

Two things the diagram is meant to settle. The sink belongs to the **driver**, not to this
crate — a handler writes into memory the caller owns, which is what makes the crate
allocation-free. And there is no edge from anything inside ``uds_services`` to
``uds_session``: session state arrives as values on ``Ctx``, through the binding.

.. needflow::
   :filter: id in ["UDSSVC_ARCH_0015", "UDSSVC_ARCH_0016", "UDSSVC_ARCH_0017", "UDSSVC_ARCH_0018"]
   :link_types: depends_on
   :align: center

Request context
---------------

.. arch:: Ctx carries one field per mandatory decision input
   :id: UDSSVC_ARCH_0015
   :depends_on: UDSSVC_ARCH_0002
   :status: draft
   :origin: derived
   :tags: seam; ctx

   Every input the pipeline needs but cannot determine from the request bytes is supplied
   by the caller as a request context. The context carries exactly the inputs the mandatory
   decision nodes of Figures 5 and 6 and the suppression rules of clause 8.7.5 read, and
   nothing else:

   .. list-table::
      :header-rows: 1
      :widths: 26 30 44

      * - Field
        - Read by
        - Supplied by
      * - addressing (physical / functional)
        - Suppression, ``UDSSVC_ARCH_0009`` rule 1
        - The binding, from the request's target address type
      * - active session
        - Figure 5 → 0x7F, Figure 6 → 0x7E
        - ``uds_session``, forwarded by the binding
      * - security level
        - Figure 5 and Figure 6 optional checks → 0x33
        - The application, forwarded by the binding
      * - authenticated
        - Figure 5 and Figure 6 → 0x34
        - The application, forwarded by the binding
      * - response-pending sent
        - Suppression, ``UDSSVC_ARCH_0009`` rule 3
        - ``uds_session``, which decides to emit 0x78

   Rationale: taking these as parameters rather than through a dependency is what makes the
   crate usable under any binding, and it avoids depending on ``uds_session``, which is
   private. The pipeline becomes a function of the request and this context, so every
   clause 8.7 rule is testable without a network, a clock or a session layer.

   **Session and security are raw sub-function values, passed through uninterpreted.**
   Naming the sessions here would mean this crate deciding what
   ``DiagnosticSessionControl``'s sub-functions mean, which is ``uds_protocol``'s to define
   and the application's to choose.

   **Addressing is this crate's own two-variant distinction, not a binding's addressing
   type.** No clause 8.7 decision reads a source or target address — only whether the
   request was functionally addressed. A binding's addressing triple is transport-shaped
   (``uds_on_ip``'s carries a DoIP logical address), so embedding one would make a
   transport a mandatory dependency of the typed layer and contradict
   ``UDSSVC_ARCH_0003``.

   **Response-pending sent is the field most easily left out, and the pipeline is
   incorrect without it.** Clause 8.7.5 guards both suppression rules on whether a 0x78 has
   already gone out, and states that after one has, the final response shall be sent even
   for a functionally addressed request that would otherwise be silenced. A dispatcher
   without this input answers a functionally addressed request with silence where the
   standard requires a final negative response — and because the failure needs a slow
   handler *and* functional addressing to appear, it will not show up in ordinary testing.

   Peer identity is deliberately absent. An application that keys state per tester — a
   security-access attempt counter, say — holds that state in its own server type, where it
   already holds everything else.

Outcome
-------

.. arch:: Dispatch reports an outcome; only sink failures are errors
   :id: UDSSVC_ARCH_0016
   :depends_on: UDSSVC_ARCH_0009
   :status: draft
   :origin: derived
   :tags: seam; outcome

   Dispatch reports whether a response was written and should be transmitted, or whether
   nothing is to be sent. A negative response is the first of those, not an error. The
   error type is the sink's own.

   Rationale: a negative response is a normal, specified outcome of clause 8.7 expressed in
   the written bytes — a server answering 0x11 has succeeded at its job. Typing it as an
   error would put the most common non-trivial path through the crate into the ``Err``
   branch, and callers would learn to ignore errors.

   Silence must be a distinguishable outcome rather than "wrote nothing", because the
   caller has to tell "clause 8.7 requires no response" apart from "the handler produced an
   empty response" apart from "something went wrong". Only the first is a reason not to
   transmit.

   Genuine transport failures are the binding's concern and never reach a handler.

Response sink
-------------

.. arch:: A response is written into a caller-supplied sink
   :id: UDSSVC_ARCH_0017
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: seam; no_std

   A handler writes its response into an ``embedded_io::Write`` sink supplied by the
   caller, rather than returning an owned response. No public type carries a ``Vec`` or a
   ``String``, and the crate builds under ``no_std`` without ``alloc``.

   Rationale: allocation-freedom cannot be retrofitted, because the signatures that make an
   API alloc-free are the ones callers depend on. Designing it in from the start costs
   little; adding it later is a breaking change to every handler in every application.

   The sink is a **generic parameter, not ``dyn``**. That costs object safety — there is no
   ``Box<dyn UdsServer>``, and a driver is generic over the handler type instead. For a
   crate that must build without ``alloc``, where boxing is unavailable anyway, it is the
   right trade, but it is a deliberate one and should be revisited if a dynamic service
   registry is ever wanted.

   One consequence to design against rather than discover: a sink can fail mid-response,
   after some bytes are already written. What a partially written response means at the
   byte seam — whether the binding must discard it, and whether this crate must avoid
   writing until it can complete — is not settled. See :doc:`open-questions`.

   ``no_std`` freedom is verified on a bare-metal target, not by
   ``--no-default-features`` on a hosted one. Only a ``*-none`` target proves ``std`` has
   not crept back in through a dependency.

Binding adapter
---------------

.. arch:: The binding declares the byte seam; this crate implements it
   :id: UDSSVC_ARCH_0018
   :depends_on: UDSSVC_ARCH_0003; UDSSVC_ARCH_0015; UDSSVC_ARCH_0016
   :status: draft
   :origin: derived
   :tags: seam; transport

   A byte seam is necessarily transport-shaped — it is where a binding hands off — so each
   binding declares its own, and this crate implements whichever one its enabled feature
   selects. Behind the ``doip`` feature it provides a blanket implementation of
   ``uds_on_ip``'s request-handler trait for any assembled typed server, converting that
   crate's context into the context of ``UDSSVC_ARCH_0015``.

   Rationale: this keeps the dependency edge pointing outward from this crate
   (``UDSSVC_ARCH_0002``) and keeps the binding ignorant of what a service is. The adapter
   is small by construction: if it ever needs to make a decision, that decision belongs on
   one side of the seam or the other, not in the conversion.

   The conversion is where a mismatch between this crate's context and a binding's becomes
   visible, which is its second purpose. ``uds_on_ip``'s context today carries an
   addressing triple, the active session and the security level; it carries neither the
   authentication state nor the response-pending flag that ``UDSSVC_ARCH_0015`` requires.
   Those fields have to be added there, and ``uds_session`` has to expose the second of
   them, before the adapter can be written. This is the reason the stack is being iterated
   as a whole rather than crate by crate.
