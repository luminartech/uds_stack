Position in the stack
=====================

Where this crate sits, what it depends on, and what a transport swap replaces.

.. uml::
   :align: center
   :caption: The stack, one package per ISO document. Dashed edges are the message
             vocabulary rather than a layer boundary.

   @startuml
   package "application" {
     [tester application\n(client role)] as CLI
     [ECU application\n(server role)] as APP
   }

   package "ISO 14229-1 cl. 8.7" {
     [uds_services] as SVC
   }

   package "ISO 14229-5" {
     [uds_on_ip] as OIP
   }
   package "ISO 14229-3" {
     [uds_on_can] as OCAN
   }

   package "ISO 14229-2" {
     [uds_session] as SESS
   }

   package "ISO 13400-2" {
     [simple_doip] as DOIP
   }
   package "ISO 15765-2" {
     [ISO-TP] as TP
   }

   package "ISO 14229-1 messages" {
     [uds_protocol] as PROTO
   }

   CLI -down-> SVC : typed requests
   APP -down-> SVC : typed service handlers
   SVC -down-> OIP : byte seam + Ctx
   SVC -down-> OCAN : byte seam + Ctx
   OIP -down-> SESS
   OCAN -down-> SESS
   OIP -down-> DOIP
   OCAN -down-> TP

   PROTO .up.> SVC
   PROTO .up.> OIP
   PROTO .up.> OCAN
   @enduml

``uds_protocol`` is drawn to one side with dashed edges because it is not a layer: it is
the message vocabulary this crate and both bindings speak. Nothing is above or below it.

``uds_on_can`` and ISO-TP appear because the shape of the stack is the argument for the
byte seam being where it is. Neither exists yet.

Both application roles meet the stack at the same crate, which is ``UDSSVC_ARCH_0019``.
They are drawn as separate components because they are separate programs — a tester and an
ECU — not two faces of one. What they share is the identifier vocabulary between them,
which is the one thing the diagram cannot show.


Scope
-----

.. arch:: The crate owns ISO 14229-1's behaviour; uds_protocol owns its format
   :id: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope

   Rationale: the stack is organised one crate per ISO document, so "does this belong here?"
   must be answerable by checking which document specifies the behaviour. ISO 14229-1 is the
   one document too large for that rule to settle on its own, and it is split in two:

   * **``uds_protocol`` owns the format** — bits, bytes, and which messages are valid.
   * **``uds_services`` owns everything else in ISO 14229-1** — the behaviour.

   That is the whole boundary, and it leaves no clause unassigned. It is the only exception
   to the one-crate-per-ISO-document rule in the stack: ISO 14229-2 is ``uds_session``,
   ISO 14229-5 is ``uds_on_ip``, ISO 13400-2 is ``simple_doip``, each whole.

   **Owning a behaviour means implementing it or defining the seam where it attaches.** Much
   of ISO 14229-1's behaviour is unownable by a library — what ``ECUReset`` resets is a
   property of one ECU — so this crate implements what is common to every server and defines
   the interface through which an application supplies what is specific to one. That is the
   pattern in ``UDSSVC_ARCH_0012``'s service traits, ``UDSSVC_ARCH_0014``'s identifiers,
   ``UDSSVC_ARCH_0026``'s ``split_record`` and ``UDSSVC_ARCH_0033``'s per-service
   permission: the concern is owned here, the instance is delegated. A clause with no seam
   and no implementation is unbuilt, not out of scope.

   What is excluded is what another document specifies: message encoding, timers, session
   state, and transport.

   **Clause 8.7 is the densest part of the scope and the reason the crate exists**, not its
   boundary. It is the dispatch-and-negative-response state machine of :doc:`dispatch`, it
   is what most implementations get wrong, and it is what is worth centralising — but the
   set already reaches clause 7 for the service access point (``UDSSVC_ARCH_0020``,
   ``UDSSVC_ARCH_0021``), clauses 8.5 and 8.6 to construct a response
   (``UDSSVC_ARCH_0010``), clause 11.2 for the extent of a data record
   (``UDSSVC_ARCH_0008``, ``UDSSVC_ARCH_0026``) and Annex A for the usage rules of a
   negative response code (``UDSSVC_ARCH_0032``). An earlier statement of this element read
   "clause 8.7 and nothing else", which was false of the set that existed when it was
   written.

   **In scope and not built.** Recorded here rather than in :doc:`open-questions`, because
   these are not unsettled — they are simply absent, and a reader should not have to infer
   from silence that a clause was decided against:

   * **Clause 17**, the non-volatile server memory programming process. Normative, and a
     framework with vehicle-manufacturer-specific steps, so it fits the delegation above.
     It spans many exchanges, sessions and resets, so it needs state between requests that
     the dispatch of ``UDSSVC_ARCH_0004`` has nowhere to hold, and it coordinates
     ISO 14229-2 explicitly, so it sits above ``uds_session`` rather than beside it. It is
     the largest single thing this crate does not have.
   * **Clause 16**, the security sub-layer. Its 16.1.4 general server response behaviour is
     this crate's by the same argument as clause 8.7; its 16.1.3 access flow spans
     exchanges as clause 17 does. ``UDSSVC_ARCH_0011`` files Figure 5's optional 0x38 and
     0x39 checks as caller-supplied without an ``A_Mtype`` to check against, which is where
     the gap surfaces today.

   **This bounds what the crate implements, not what it is.** ``UDSSVC_ARCH_0019`` states
   the crate's architectural role separately, because a scope statement that also carried
   that claim would stop being checkable.

   Every concern in :doc:`not-owned` is argued against this boundary.

.. arch:: The crate is the stack's integration surface, in both directions
   :id: UDSSVC_ARCH_0019
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope; roles

   Rationale: left unwritten, this is the property most likely to be designed against by
   accident. Beyond implementing clause 8.7, this crate is where an application meets the
   diagnostic stack. It defines the interface by which a server integrates a stack — the
   handler traits, the assembly, and the dispatch of :doc:`service-traits` — and the set of
   requests available to a client application (:doc:`client-surface`). No other crate in
   the stack offers either.

   Three consequences follow, and each is visible elsewhere in this document:

   * **Both roles share one identifier vocabulary** (``UDSSVC_ARCH_0024``). An application
     that defines its identifiers once must be able to build an ECU and a tester from
     them.
   * **Both roles are transport-agnostic.** Whatever is portable across bindings is
     portable for both, and ``UDSSVC_ARCH_0003``'s limit — that the diagnostic
     conversation moves and connection setup does not — applies to each.
   * **The crate is the last layer that understands UDS.** Everything below it handles
     bytes. That is why interpreting a negative response is here on the client side
     (``UDSSVC_ARCH_0021``) and writing one is here on the server side
     (``UDSSVC_ARCH_0010``), and it is why the layer below explicitly declines both.

   Stated as a goal: **an application using this stack should not need to care about UDS
   much at all** — in either role. It defines its identifiers, implements the services it
   serves, calls the services it needs, and the rest follows.

Dependencies
------------

.. arch:: uds_protocol is the only mandatory dependency
   :id: UDSSVC_ARCH_0002
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope; dependencies

   Rationale: this crate must stay usable under any binding, and must not carry a hard dependency on
   ``uds_session`` — which is currently private and would make this crate unpublishable.
   It meets both by taking session state and security level as *parameters* rather than
   through a dependency (``UDSSVC_ARCH_0015``): this crate depends on ``uds_protocol`` for
   the request and response message types it dispatches over, and on ``embedded-io`` for
   the response sink, and shall not depend on ``uds_session``, ``uds_on_ip``,
   ``uds_on_can`` or ``simple_doip`` other than through an optional, additive feature.

   The dependency edge on a binding points one way and outward: ``uds_services →
   uds_on_ip``, optional. The binding stays ignorant of what a service is. This is the
   ``tower``/``hyper`` arrangement, and it is what lets a typed server move to CAN
   unchanged.

.. arch:: Transport bindings are optional and additive
   :id: UDSSVC_ARCH_0003
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope; transport

   Rationale: a ``ReadDataByIdentifier`` handler that knows how to fetch an identifier has nothing to
   say about IP, so the ergonomic layer — the part application authors actually touch —
   must be free to follow a server to CAN unchanged. Each transport binding is therefore
   selected by a Cargo feature that pulls in that binding and implements its byte seam.
   Enabling none leaves a typed server that dispatches over ``uds_protocol`` messages;
   enabling one adds the adapter described by ``UDSSVC_ARCH_0018``.

   .. code-block:: toml

      [features]
      doip  = ["dep:uds_on_ip"]
      docan = ["dep:uds_on_can"]

   **What is not portable, stated so nobody is surprised.** The diagnostic *conversation*
   moves between transports; *connection setup* does not. DoIP has TCP connections,
   routing activation and vehicle identification; CAN has none of them. Defining
   identifiers and implementing handlers is portable. Establishing and configuring the
   link is a real seam in the application, and no arrangement of this crate removes it.

How a request crosses the stack
-------------------------------

The layering diagram says what the pieces are. This says what happens, and it is where the
seams earn their keep.

.. uml::
   :align: center
   :caption: One physically addressed request reaching a server, from the wire to a
             handler and back. The client-side path is in :doc:`client-surface`.

   @startuml
   autonumber "<b>[0]"
   hide footbox

   actor tester as T
   box "binding" #F4F7FA
     participant "simple_doip" as D
     participant "uds_on_ip" as I
     participant "uds_session" as S
   end box
   participant "uds_services" as V
   participant "application\nhandler" as H

   T -> D : DoIP diagnostic message
   D -> I : payload bytes + addressing
   I -> S : indicate request received
   S --> I : active session, security level
   I -> V : handle(ctx, request bytes, sink)
   activate V

   V -> V : decode, preconditions,\nsub-function, data parameters
   V -> H : typed handler call (awaited)
   activate H
   H --> V : Ok, or a negative response code
   deactivate H
   V -> V : suppression gate
   V --> I : Responded, or Suppress
   deactivate V

   alt Responded
     I -> D : response bytes
     D -> T : DoIP diagnostic message
   else Suppress
     I -> I : send nothing
   end
   @enduml

Step 3 is the one to look at. Session and security reach this crate as *values*, from the
binding, having been read from ``uds_session``. There is no edge from ``uds_services`` to
``uds_session`` on any diagram in this document, and ``UDSSVC_ARCH_0002`` is why.

The final ``alt`` is not error handling. Both branches are specified outcomes of clause
8.7. Which one applies depends on the addressing mode, which arrives on ``Ctx``, and on
whether this dispatch offered a response-pending (``UDSSVC_ARCH_0032``) — neither of which
inspecting the request bytes would reveal.

.. arch:: An async runtime is assumed; none is depended on
   :id: UDSSVC_ARCH_0030
   :depends_on: UDSSVC_ARCH_0002
   :status: draft
   :origin: derived
   :tags: scope; dependencies; async

   Rationale: the layers below are already asynchronous and not optionally so: ``simple_doip``'s
   ``client`` and ``server`` features each require its ``codec`` feature, which requires
   ``std`` and tokio; ``uds_on_ip``'s ``client`` and ``server`` each imply ``std`` and
   tokio in turn. A configuration with no executor cannot reach a transport, so this crate
   is not designed around one.

   Every deployment of this stack is assumed to have an async executor available — tokio
   on a host, ``embassy`` or equivalent on an embedded target. This crate exposes
   asynchronous interfaces where the direction of control requires them, and depends on no
   runtime crate: ``async`` is not a runtime, asynchronous functions in traits are stable,
   and the executor is the caller's. This crate can therefore be asynchronous at its seams
   and still carry no ``std`` requirement and no executor dependency of its own — which is
   what keeps ``UDSSVC_ARCH_0027``'s embedded build possible.

   What this assumption *buys* is recorded where it is spent:
   ``UDSSVC_ARCH_0016`` on the server side, where an asynchronous handler removes the need
   for the binding's driver to run slow handlers somewhere it can continue past, and
   ``UDSSVC_ARCH_0028`` on the client side, where awaiting is what makes one typed call
   possible instead of three.

Two different graphs
--------------------

The layering diagram and the Cargo dependency graph are not the same shape, and confusing
them is the most common way to misread this stack. At the handler seam they point in
opposite directions:

.. uml::
   :align: center
   :caption: At the handler seam, the caller and the dependent are not the same crate.

   @startuml
   [uds_services] as SVC
   [uds_on_ip] as OIP

   OIP -[#4A6E8A]-> SVC : <b>calls</b>\nhands over bytes + Ctx
   SVC -[#C0392B,dashed]-> OIP : <b>depends on</b>\nimplements its seam, behind a feature

   legend right
     |= |= edge |
     |<back:#4A6E8A>   </back>| processing: who calls whom |
     |<back:#C0392B>   </back>| Cargo: who names whom in Cargo.toml |
   endlegend
   @enduml

*Protocol layering* runs application → clause 8.7 → binding → session → transport, because
that is the order a request is processed in. So ``uds_on_ip`` calls ``uds_services``.

*Cargo dependencies* run the other way here: ``uds_services`` optionally depends on
``uds_on_ip``, not the reverse. The binding declares a byte seam and knows nothing about
services; this crate implements it.

That inversion is deliberate and is what makes the arrangement work — it is how ``tower``
relates to ``hyper``. A layer being below another in processing order says nothing about
which crate names the other in its manifest, and the two questions have different answers
here.
