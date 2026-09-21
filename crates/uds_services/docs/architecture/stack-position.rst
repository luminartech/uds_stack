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
   SVC -down-> OIP : UdsTransport
   SVC -down-> OCAN : UdsTransport
   SVC -right-> SESS : drives
   OIP -down-> DOIP
   OCAN -down-> TP

   PROTO .up.> SVC
   PROTO .up.> OIP
   PROTO .up.> OCAN
   @enduml

``uds_session`` is drawn to the side of this crate rather than under a binding, because
``UDSSVC_ARCH_0040`` has this crate own the ``Session`` instance: it supplies every input and
drains every action. A binding never touches it.

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

   * **Clause 17**, the non-volatile server memory programming process — on the *client*
     side only. ``UDSSVC_ARCH_0039`` settles where it lands: the clause is a sequence the
     client is required to follow, it binds the server directly three times in 625 lines,
     and two of those three are ECU properties rather than protocol behaviour. The server
     side is discharged by ``UDSSVC_ARCH_0036``, ``UDSSVC_ARCH_0037`` and
     ``UDSSVC_ARCH_0038``. What is not built is the client orchestration, and only its
     standardized steps are common enough across vehicle programmes to be worth building.
   * **The per-service negative response evaluation sequences**, fifteen normative figures
     the "Specific SID CHECK" box of Figures 5 and 6 hands off to. ``UDSSVC_ARCH_0042``
     records that the ordering is normative and this crate's, and that the seam by which a
     service trait expresses it is not yet designed. Until it is, a handler chooses its own
     code and its own order, which is the arrangement this crate exists to remove.
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

.. arch:: The stack owns every protocol concern it can model
   :id: UDSSVC_ARCH_0034
   :depends_on: UDSSVC_ARCH_0001; UDSSVC_ARCH_0019
   :status: draft
   :origin: derived
   :tags: scope; roles

   Where ISO 14229-1 specifies behaviour, this crate implements it. Where ISO 14229-1
   defers behaviour to the vehicle manufacturer or to the ECU, this crate implements the
   *shape* the standard does fix and delegates only the part it defers — through an
   interface designed so that the compliant implementation is the easy one to write.

   Rationale: the crate exists because clause 8.7's rules are easy to get subtly wrong and
   the failures are invisible against a cooperative client. That argument does not stop at
   clause 8.7. Every protocol rule left to an integrator is reimplemented once per vehicle
   programme and got wrong in the same ways each time, and ``UDSSVC_ARCH_0001`` has already
   made the whole of ISO 14229-1's behaviour this crate's. A concern is delegated because
   the standard forces it, never because delegating is easier here.

   The test, applied to any behaviour in scope:

   #. **Does the standard fix it?** Then it is implemented here.
   #. **Does the standard explicitly defer it?** Then only the deferred part is delegated,
      and the fixed part around it stays.
   #. **Can a caller be non-compliant without noticing?** Then the seam is wrong, whatever
      the documentation says.

   ISO 14229-1:2020 10.4 is the worked example, because it does both things in one clause.
   The *shape* is fixed: ``requestSeed`` precedes ``sendKey``, "an invalid key shall require
   the client to start over from the beginning", a server already unlocked answers
   ``requestSeed`` with a zero seed and "shall never send an all zero seed for a given
   security level that is currently locked", and "only one security level shall be active at
   any instant of time". The *policy* is disclaimed as plainly: "The vehicle manufacturer
   shall select if the delay timer is supported", and NOTE 8 leaves "the management of
   failed attempts (e.g. maximum number of attempts, delay, etc.) ... to the vehicle
   manufacturer's discretion". So the sequence is this crate's and the application supplies
   a seed, a judgement on a key, and an attempt policy. An application that implements those
   three cannot get the ordering, the restart rule or the zero-seed probe wrong, because it
   never sees them.

   **Misuse-resistance is a design obligation, not a documentation one.** Where an error can
   be made unrepresentable it is, in preference to warning against it: ``UDSSVC_ARCH_0033``
   gives its constant no default so omission fails to compile rather than silently choosing,
   and ``UDSSVC_ARCH_0014`` makes ``from_u16`` fallible so an unsupported identifier cannot
   reach a handler. Documentation is what remains after that, not a substitute for it.

.. arch:: This crate drives the stack
   :id: UDSSVC_ARCH_0040
   :depends_on: UDSSVC_ARCH_0019; UDSSVC_ARCH_0034
   :status: draft
   :origin: derived
   :tags: scope; roles; driver

   This crate owns the run loop. It supplies ``uds_session`` with timestamps and drains its
   outputs, calls a transport to send and receive bytes, dispatches inbound requests to the
   consuming application's handlers, and conducts the client's exchanges. No other crate in
   the stack contains a driver, and a consuming application writes none.

   **Two words, because the stack has two things and one name for them.** The **application
   layer** is ISO 14229-1's, which is this crate; ISO 14229-2 names it as the session
   layer's service user, and ``uds_session`` uses "application" in exactly that sense. The
   **consuming application** is the ECU or tester program written against this crate's
   interface — it defines identifiers, implements service callbacks, and calls the client.
   Where this set previously wrote "the application" it meant the second.

   Rationale: ISO 14229-2 defines a service interface between the session layer and its
   user and names that user as the ISO 14229-1 layer. An arrangement in which a third party
   sits between them — turning the crank on the session layer and calling into this crate —
   describes a component no standard mentions, and it is the component that went a full
   design cycle with no owner: ``uds_session`` declined it as out of its sans-io scope,
   ``uds_on_ip`` states it "is not implemented in this prototype", and this crate's scope
   element kept it out. Collapsing the service user and the driver into one removes the
   vacancy rather than assigning it.

   ``UDSSVC_ARCH_0034`` reaches the same place from the other direction. A run loop every
   integrator writes is the largest protocol concern the stack was leaving to them, and its
   obligations are UDS-semantic rather than transport-semantic: whether a response-pending
   is admissible needs to know which services the server supports, clause 8.7.6's occupancy
   of the diagnostic protocol instance is a clause 8.7 rule, and coordinating ``tP2_Server``
   against a running handler is ISO 14229-1 meeting ISO 14229-2. A binding that owned the
   loop would have to understand all three.

   Three consequences.

   * **The consuming application implements callbacks and nothing else.** That is what
     ``UDSSVC_ARCH_0019``'s goal — "an application using this stack should not need to care
     about UDS much at all" — has meant all along; it was not achievable while a driver
     remained unwritten.
   * **A binding becomes a transport implementation.** ``uds_on_ip`` supplies framing,
     connection setup, routing activation and vehicle identification, and implements the
     seam this crate calls. It contains no driver and no notion of a service.
   * **This crate needs time**, which it did not before. It comes from the transport rather
     than from a seam of its own — ``UDSSVC_ARCH_0041``.

   **What "drains its outputs" means concretely**, since this element asserted the loop's
   existence before the contract on the other side of it was fixed. ``uds_session`` does not
   offer a ``poll()`` returning one output at a time. Each input — ``t_data_ind``,
   ``s_data_req``, ``tick`` — returns a *reaction*: an iterator over that input's outputs,
   which the driver drains with ``Iterator::by_ref`` and then closes with ``finish()``,
   whose return value is the input's verdict. Drain, then finish; the two are not
   interchangeable and neither is optional, because the outputs are what must reach the
   transport and the verdict is what says whether a submission was accepted at all
   (``UDSSVC_ARCH_0031``'s refusal, which ``UDSSVC_ARCH_0009``'s gate depends on). Every
   ``Transmit`` is sent from *inside* the drain rather than after it, so the last output is
   not the only one that reaches the wire.

   Four properties of the loop body are load-bearing and none of them is obvious from the
   description above. They are recorded here because each is a borrow-order constraint that
   reads as an arbitrary stylistic choice, and each would be "simplified" away by a reader
   who did not know what it was for.

   1. **Every transport query is sampled before any future holding ``&mut transport``
      exists.** ``outbound_max`` and ``now`` take ``&self``, but a live ``next_event``
      future refuses even shared access — so the deadline, the outbound bound and the
      timestamp are all read first. One timestamp per iteration follows, which is the shape
      ``uds_session`` asks for anyway: a timestamp accompanies every input, and two reads in
      one iteration could disagree.
   2. **The indication is taken out of the drain before ``finish()`` is called.** An output
      outlives the reaction that yielded it, which is what lets dispatch run with
      ``&mut session`` free again. This is not a tidiness point: dispatching while the
      reaction still borrows the session would leave no way to submit a 0x78 during the
      handler, and ``UDSSVC_ARCH_0031``'s window would not exist.
   3. **The select is a loop over ``handler.as_mut()``, not a one-shot.** A plain
      ``select2(handler, waiting)`` moves the handler in and drops it the moment the
      deadline wins — which is the exact opposite of what a response-pending is for, since
      0x78 means *the handler is still running*. Re-borrowing through ``Pin::as_mut`` lets
      one handler survive arbitrarily many deadlines and arbitrarily many 0x78s.
   4. **Two ``pin!`` scopes, because ``pin!`` binds to the enclosing block.** The handler is
      scoped so it drops before the sink is read for the bytes to transmit; the waiting
      future is scoped per iteration so it drops before the 0x78 path needs
      ``&mut transport`` to send. Without the inner scope the response-pending cannot be
      transmitted while the thing that detected the deadline is still alive.

   The loop body is consequently long and is not split into helpers. That is deliberate:
   the four properties above are properties of *one* borrow order, and a helper taking
   ``&mut self.session`` and ``&mut self.transport`` across a call boundary is precisely
   what the scoping exists to avoid.

   What this costs is honesty about the server side's shape. The dispatch pipeline of
   ``UDSSVC_ARCH_0004`` remains a pure function of a request and its context, and is still
   testable as one. The loop around it is not: it awaits a transport and a clock. Both are
   seams rather than dependencies, so a test binds a fake of each and needs no network and
   no executor of its own — but the claim that this crate performs no I/O is retired here,
   and it should be retired plainly rather than qualified away.

Dependencies
------------

.. arch:: The dependencies are uds_protocol, uds_session and the codec — never a binding
   :id: UDSSVC_ARCH_0002
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope; dependencies

   This crate depends on ``uds_protocol`` for the message types it dispatches over, on
   ``uds_session`` for the ISO 14229-2 vocabulary and the application-facing seams it
   declares, and on ``automotive-wire-codec`` for the response sink. It shall not depend on
   ``uds_on_ip``, ``uds_on_can`` or ``simple_doip`` at all — not optionally, and not behind
   a feature.

   Rationale: a typed server must follow its application to any transport unchanged, so a
   binding cannot appear in this crate's dependency list in any form. It does not need to: this
   crate declares ``UDSSVC_ARCH_0029``'s ``UdsTransport`` and a binding implements it, so the
   Cargo edge points from the binding to here and ``uds_on_can`` becomes additive with no change
   here at all.

   **An intermediate version had these seams declared by ``uds_session``**, on the ground that
   ISO 14229-2 specifies the application-facing service interface. Right about the document,
   wrong about the component — that interface is between the session layer and *this crate*, so
   with ``UDSSVC_ARCH_0040`` there is no third party for it to sit between. ``uds_session``
   declares no outward trait at all now. The dependency on it stands for the ISO 14229-2
   vocabulary and the state machine this crate drives.

   **An earlier version of this element forbade depending on ``uds_session``**, on the
   ground that it was private and would make this crate unpublishable, and took session and
   security state as parameters to avoid it. That constraint is gone — ``uds_session`` is
   public — and the parameter-passing it motivated has been overtaken from the other
   direction: this crate implements ``DiagnosticSessionControl``, ``SecurityAccess`` and
   ``Authentication``, so under ``UDSSVC_ARCH_0035`` it owns that state rather than
   receiving it.

   What survives from that reasoning is the part that was never about publishing: two
   crates tracking the same state is how they come to disagree, and no arrangement here
   should produce one.

.. arch:: Transport bindings are optional and additive
   :id: UDSSVC_ARCH_0003
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: scope; transport

   Rationale: a ``ReadDataByIdentifier`` handler that knows how to fetch an identifier has nothing to
   say about IP, so the ergonomic layer — the part consuming-application authors actually touch —
   must be free to follow a server to CAN unchanged. It is, because no binding appears here in
   any form: this crate declares the single seam (``UDSSVC_ARCH_0029``) and a binding implements
   it. Adding ``uds_on_can`` changes nothing in this crate and requires no feature.

   **"Binding" now means a transport implementation**, not a host for a driver. Under
   ``UDSSVC_ARCH_0040`` a binding supplies framing, connection setup, routing activation and
   vehicle identification, implements ``UdsTransport``, and contains no run loop and no notion
   of a service. Earlier versions of this element used the word for a component that also drove
   the stack.

   **An earlier version of this element gave each binding a Cargo feature** —
   ``doip = ["dep:uds_on_ip"]`` and a ``docan`` beside it — each pulling in that binding and
   implementing its own byte seam. That followed from every binding declaring a seam of its
   own, which is no longer the case. One seam, one implementation, no features, and nothing
   to enable or forget.

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
   participant "simple_doip" as D
   participant "uds_on_ip" as I
   box "uds_services" #F4F7FA
     participant "driver loop" as L
     participant "uds_session" as S
     participant "dispatch" as V
   end box
   participant "consuming\napplication handler" as H

   T -> D : DoIP diagnostic message
   D -> I : payload bytes + addressing
   I --> L : next_event(buffer, deadline)\n→ DataInd { ai, len }
   activate L
   L -> S : t_data_ind(now, ai, bytes, ..)
   S --> L : a reaction (an iterator\nover this input's outputs)
   loop drain with Iterator::by_ref
     S --> L : ServerOutput::Indicate { ai, data }
   end
   L -> S : finish()
   S --> L : this input's verdict
   L -> V : dispatch(request bytes, sink)
   activate V

   V -> V : decode, preconditions,\nsub-function, data parameters
   V -> H : typed handler call (awaited)
   activate H
   H --> V : Ok, or a negative response code
   deactivate H
   V -> V : suppression gate
   V --> L : Responded::Yes,\nor Responded::Suppressed
   deactivate V

   alt Responded::Yes
     L -> S : s_data_req(now, ai, response bytes, ..)
     S --> L : a reaction
     loop drain with Iterator::by_ref
       S --> L : ServerOutput::Transmit { ai, data }
       L -> I : t_data_req(ai, data)
       I -> D : response bytes
       D -> T : DoIP diagnostic message
     end
     L -> S : finish()
     S --> L : accepted, or refused
   else Responded::Suppressed
     L -> L : send nothing
   end
   deactivate L
   @enduml

The box is the point. Everything between the transport handing up an event and the transport
being handed bytes back is inside this crate: it reads the transport, turns the session
layer's crank, dispatches, and transmits. Nothing calls into it, which is
``UDSSVC_ARCH_0018``, and session state never arrives as a parameter because this crate holds
it (``UDSSVC_ARCH_0035``) — which is why ``dispatch`` takes the request bytes and a sink and
nothing else.

**Two details of the drain are drawn deliberately, because both are load-bearing**
(``UDSSVC_ARCH_0040``). An input does not return one output: it returns a *reaction*, which
the driver drains with ``Iterator::by_ref`` and then consumes with ``finish()``, whose value
is that input's verdict — and a refused submission is exactly what ``UDSSVC_ARCH_0009``'s
suppression gate needs to know about. And ``t_data_req`` is called from **inside** the drain
rather than after it, because every ``Transmit`` must reach the transport, not only the last
one.

An earlier version of this diagram had a binding in the driving position, handing this crate
bytes and a ``Ctx`` read from ``uds_session``, and taking an ``Outcome`` back. That component
does not exist.

The final ``alt`` is not error handling. Both branches are specified outcomes of clause
8.7. Which one applies depends on the addressing mode, which arrives on ``Ctx``, and on
whether this dispatch submitted a response-pending (``UDSSVC_ARCH_0032``) — neither of which
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
   ``UDSSVC_ARCH_0016`` on the server side, where an asynchronous handler lets this crate's own
   loop keep draining session actions while a slow handler runs, and
   ``UDSSVC_ARCH_0028`` on the client side, where awaiting is what makes one typed call
   possible instead of three.

One graph, and it is worth saying so
------------------------------------

This page used to carry a section warning that the layering diagram and the Cargo dependency
graph were different shapes, and that confusing them was the most common way to misread the
stack. Under ``UDSSVC_ARCH_0040`` they are the same shape, and the warning is retired rather
than deleted because the arrangement it described was deliberate and is worth knowing was left
behind.

.. uml::
   :align: center
   :caption: Processing order and Cargo dependency now point the same way at every edge.

   @startuml
   [uds_services] as SVC
   [uds_session] as SESS
   [uds_on_ip] as OIP

   SVC -[#4A6E8A]-> OIP : <b>calls</b>\nUdsTransport
   OIP -[#C0392B,dashed]-> SVC : <b>depends on</b>\nimplements UdsTransport
   SVC -[#C0392B,dashed]-> SESS : <b>depends on</b>\ndrives the session layer

   legend right
     |= |= edge |
     |<back:#4A6E8A>   </back>| processing: who calls whom |
     |<back:#C0392B>   </back>| Cargo: who names whom in Cargo.toml |
   endlegend
   @enduml

A caller and its callee are still opposite ends of one Cargo edge — ``uds_services`` calls
``uds_on_ip`` and ``uds_on_ip`` names ``uds_services`` — which is the ordinary trait-inversion
shape, not a peculiarity of this stack. What has gone is the case where *neither* crate named
the other and both met at an interface owned by a third crate below them both.

The retired arrangement, for the record: ``uds_on_ip`` called ``uds_services`` across a
``RequestHandler`` that ``uds_session`` declared, so the caller and the callee met at an
interface owned by a crate below both of them, and neither named the other in its manifest.
That followed from a binding hosting the driver. ``UDSSVC_ARCH_0018`` records why nothing calls
into this crate now, and ``UDSSVC_ARCH_0029`` why the one seam below it is its own.
