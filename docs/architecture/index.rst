Architecture
============

How the five crates of the stack fit together: what each one owns, what it must not
acquire, the seams between them, and who owns each timer. Each crate's own design is
described separately, under :ref:`crate-architecture`.

A table, figure, clause or requirement cited on this page always names its document and
edition.

The organising idea
-------------------

ISO 14229 is not one standard but a series, and it is already layered. The stack mirrors
that layering with one crate per document, so "does this belong here?" is answered by
asking which document specifies the behaviour.

The result is reuse. A session layer that reads no clock and calls no transport serves
CAN, DoIP, K-line and simulation alike; a service dispatcher that knows how to fetch a
data identifier has nothing to say about IP. Only the crates that name a transport are
bound to one.

.. uml::
   :align: center
   :caption: The stack today. Solid edges are calls; dashed edges are the message
             vocabulary, which every layer above the wire shares.

   @startuml
   package "application" {
     [tester application\n(client role)] as CLI
     [ECU application\n(server role)] as APP
   }
   package "ISO 14229-1:2020, behaviour" {
     [uds_services] as SVC
   }
   package "ISO 14229-2:2021" {
     [uds_session] as SESS
   }
   package "ISO 14229-5:2022" {
     [uds_on_ip] as OIP
   }
   package "ISO 13400-2:2019" {
     [simple_doip] as DOIP
   }
   package "ISO 14229-1:2020, messages" {
     [uds_protocol] as PROTO
   }

   CLI -down-> SVC : typed requests
   APP -down-> SVC : typed service handlers
   SVC -right-> SESS : drives
   SVC -down-> OIP : UdsTransport
   OIP -down-> DOIP : DiagnosticEntity /\nTesterConnection

   PROTO .up.> SVC
   PROTO .up.> OIP
   @enduml

The crates
----------

.. list-table::
   :header-rows: 1
   :widths: 16 26 58

   * - Crate
     - Standard
     - Owns
   * - ``uds_protocol``
     - ISO 14229-1:2020 clauses 7 and 10 to 15
     - The encoding and decoding of every diagnostic service message. ``Request<'a>`` and
       ``Response<'a>`` borrow from the caller's buffer.
   * - ``uds_session``
     - ISO 14229-2:2021
     - The session layer: the ``S_Data`` primitives, the ``tP_Client``, ``tP3`` and
       ``tS3`` timers, and response-pending handling. It is sans-io: it takes a
       caller-supplied timestamp and returns actions to drain.
   * - ``uds_services``
     - ISO 14229-1:2020, everything but the message format
     - The point where an application meets the stack: per-service handler traits, the
       dispatch and negative-response rules of clause 8.7, the typed client calls, and the
       driver that holds the session layer and calls a transport.
   * - ``uds_on_ip``
     - ISO 14229-5:2022
     - The UDSonIP binding: the ``T_Data`` to ``DoIP_Data`` mapping, the ``tP6`` reload
       values the session layer runs on, and the profile's connection handling around a
       session change or a reset.
   * - ``simple_doip``
     - ISO 13400-2:2019
     - DoIP: framing, the TCP and UDP sockets, routing activation, alive check, vehicle
       identification and the ``DoIP_Data`` primitives.

Each also has something it must not acquire, and that is the more useful half:

- ``uds_protocol`` stays a codec: no dispatch, no policy, no session state.
- ``uds_session`` never learns which transport it runs over, and never reads a clock.
  ISO 14229-2:2021 9.1.2 splits ``tP_Client``'s reload parameters by whether the transport
  offers ``T_DataSOM.ind``; the session layer holds a *default* and an *enhanced* pair, and
  the binding decides which values those are.
- ``uds_services`` declares the transport seam and implements no transport. Nothing on that
  seam is DoIP-shaped, so a CAN binding implements the same methods.
- ``uds_on_ip`` never learns what a service is. It reads exactly four things of a message:
  a positive ``DiagnosticSessionControl`` or ``ECUReset`` response, the
  suppressPosRspMsgIndicationBit, and the response-pending negative response. Those are
  ISO 14229-5:2022 clause 8 rules keyed on a service, not service semantics.
- ``simple_doip`` never learns what a UDS message means. A diagnostic message's payload is
  an opaque byte string to it.

``automotive-wire-codec``, outside this repository, provides the zero-copy ``no_std`` codec
traits ``uds_protocol`` and ``simple_doip`` are built on.

The five crates release in lockstep to crates.io: one version for the stack, published
bottom-up in dependency order.

When and what
-------------

Several questions about where a behaviour belongs have the same answer:

   **ISO 14229-5 decides *when*. ISO 13400-2 performs *what*.**

- **Reconnect and repeat routing activation** (ISO 14229-5:2022 REQ 7.8, REQ 7.10):
  ``uds_on_ip`` sequences it and ``simple_doip`` performs it. Routing activation is
  ISO 13400-2's procedure, sent by the client entity, so only a tester reconnects.
- **Close after a positive response** (REQ 7.9, REQ 7.11): ``uds_on_ip`` decides and
  ``simple_doip`` closes. REQ 7.11's close follows every positive ``ECUReset`` response;
  REQ 7.9's follows a positive ``DiagnosticSessionControl`` response only where the server
  leaves the software it is running. That fact is the server's own, so ``uds_services``
  states it and ``uds_on_ip`` decides what follows.
- **Periodic responses** (REQ 7.7, REQ 7.16): ``uds_on_ip`` chooses payload type
  ``0x8004``, which ISO 13400-2:2019 does not define, and ``simple_doip`` sends it.
- **Which ``tP_Client`` reload pair** (ISO 14229-2:2021 REQ 5.11): ``uds_on_ip`` chooses
  and ``uds_session`` runs the timer.

The rule also says what must not happen. ``uds_services`` never decides when a connection
closes, because knowing that REQ 7.9 or REQ 7.11 requires a close is ISO 14229-5 knowledge.
``simple_doip`` never recognises a service identifier to decide for itself. A crate that
must know *whether* to act has acquired the requirement, whether or not it also performs
it.

The seams
---------

Each seam is declared by the crate that calls through it, so every Cargo edge runs from
the implementor to the declarer, and a caller never depends on its callee's choice of
binding.

**Transport seam: ``uds_services`` to a binding.** ``uds_services`` declares
``UdsTransport`` and ``TransportEvent``; ``uds_on_ip`` implements them over DoIP. An event
borrows the driver's buffer, never the transport, so a server can answer the request it
has just received. ``ClientTransport`` adds ``close`` for the client role alone; a server
is reconnected to and never reconnects, so the seam carries no ``reconnect``.

**Connection seam: ``uds_on_ip`` to ``simple_doip``.** The sockets are ISO 13400-2's, so
``simple_doip::service`` declares this seam, with no I/O in it: ``DiagnosticEntity``, a
whole entity behind one event stream, for a server; ``DiagnosticConnection`` and
``TesterConnection``, one connection a tester can replace and end, for a client. This is
the one seam declared by the callee, because ``uds_on_ip`` already consumes
``simple_doip``'s addressing and the edge cannot run the other way.

**``uds_session`` is vocabulary, not a seam.** ``uds_on_ip`` names ``uds_session``'s
addressing, ``SResult``, ``Timestamp`` and ``Reloads``, and calls none of its behaviour.
``uds_services`` holds the session layer by value, supplies its inputs and drains its
actions. ISO 14229-5:2022 specifies content above and below the session layer, but the
crate implementing both halves sits wholly below it.

The seams in detail, and the design of the two bindings, are in each crate's architecture.

Timing ownership
----------------

Every timer has exactly one owner, and every timing rule exactly one decider.

.. list-table::
   :header-rows: 1
   :widths: 30 18 52

   * - Timer
     - Owner
     - Notes
   * - ``tP_Client``
     - ``uds_session``
     - One per logical channel, physical and functional alike (ISO 14229-2:2021 REQ 5.26,
       Table 7). Starts on ``T_Data.conf`` and stops on ``T_Data.ind``
       (ISO 14229-2:2021 REQ 5.9, REQ 5.10), so over DoIP it starts on the entity's
       acknowledgement, not on the send.
   * - Default and enhanced reload values
     - ``uds_on_ip``
     - ``tP6_Client_Max`` and ``tP6*_Client_Max``, because DoIP has no ``T_DataSOM.ind``
       (ISO 14229-2:2021 REQ 5.11).
   * - ``tP3_Client_Phys``, ``tP3_Client_Func``
     - ``uds_session``
     - One per physical and per functional channel (ISO 14229-2:2021 REQ 5.26, Table 7).
       The minimum spacing before the next request when none is required.
   * - ``tS3_Client``, ``tS3_Server``
     - ``uds_session``
     - One ``tS3_Server`` per server; a client needs one per point-to-point
       communication (ISO 14229-2:2021 REQ 5.26, Table 8).
   * - ``tP2_Server``, ``tP2*_Server``
     - ``uds_session``
     - Session timers, kept with the others rather than with the service that is slow.
   * - ``tP4_Server``
     - ``uds_services``
     - A performance requirement on the application, not a timer to run.
   * - TCP connect and routing-activation timeouts
     - ``simple_doip``
     - Transport-level, with no UDS meaning.

**Who emits response-pending.** A handler that is still working is not returning, so it
cannot also watch a clock. ``uds_session`` owns ``tP2_Server`` and decides when a
response-pending is due; ``uds_on_ip`` transmits it; ``uds_services`` runs the handler
alongside the event loop, so the timer fires while the handler works.

Two graphs
----------

The protocol layering and the Cargo dependency graph are not the same shape. Reading one
as the other is the easiest way to misread the design.

The layering is the standards': ISO 14229-5 specifies an application profile above the
session layer and a transport mapping below it, and ``uds_on_ip`` implements both. The
Cargo graph has the driver, ``uds_services``, above all three, which is why it has no
cycle and needs no feature gate to avoid one.

.. uml::
   :align: center
   :caption: Cargo dependencies, read left to right in publication order. Every edge is
             unconditional, and ``uds_on_ip`` is a leaf.

   @startuml
   left to right direction
   [automotive-wire-codec] as AWC
   [uds_protocol] as UP
   [simple_doip] as SD
   [uds_session] as US
   [uds_services] as USVC
   [uds_on_ip] as UOI

   AWC --> UP
   AWC --> SD
   AWC --> USVC
   UP --> USVC
   UP --> UOI
   US --> USVC
   US --> UOI
   SD --> UOI
   USVC --> UOI
   @enduml

What a transport swap replaces
------------------------------

A binding is not a layer swapped within the stack. It is a complete path from ``A_Data``
to the wire, attached below the shared driver, so swapping replaces the binding together
with its transport while the driver, session, message and service crates stay. For
application code the swap point is the ``transport =`` parameter of ``uds_services``'
assembly macro.

.. uml::
   :align: center
   :caption: The DoIP binding as built, and a CAN binding as it would attach (dashed).

   @startuml
   [Application\ntyped service handlers] as APP
   [uds_services\nISO 14229-1 behaviour, driver\ndeclares UdsTransport] as USVC
   [uds_session\nISO 14229-2] as US
   [uds_on_ip\nISO 14229-5] as UOI
   [uds_on_can\nISO 14229-3] as UOC
   [simple_doip\nISO 13400-2] as SD
   [ISO 15765-2 crate] as CAN

   APP -down-> USVC
   USVC -right-> US : holds by value
   UOI -up-> USVC : impl UdsTransport
   UOC .up.> USVC : impl UdsTransport
   UOI -down-> SD
   UOC .down.> CAN
   @enduml

The diagnostic conversation is portable; connection setup is not. DoIP has TCP
connections, routing activation, vehicle identification and alive check, and CAN has none
of them, so establishing the link is the application's, through the binding's own API.
``uds_on_can`` and an ISO 15765-2 crate do not exist; they show where one would attach.

.. _crate-architecture:

Each crate's architecture
-------------------------

.. toctree::
   :maxdepth: 1

   uds_services/index

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Crate
     - Where its design is described
   * - ``uds_services``
     - :doc:`uds_services/index`
   * - ``uds_on_ip``
     - ``crates/uds_on_ip/ARCHITECTURE.md``, shipped with the package
   * - ``simple_doip``
     - ``crates/simple_doip/ARCHITECTURE.md``, shipped with the package
   * - ``uds_session``
     - :doc:`../requirements/index`, and its README
   * - ``uds_protocol``
     - its README
