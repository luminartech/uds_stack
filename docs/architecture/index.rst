Architecture
============

How the six crates of the stack fit together: what each one owns, what it must not
acquire, the seams between them, and who owns each timer. Each crate's own design is
described separately, under :ref:`crate-architecture`.

A table, figure, clause or requirement cited on this page always names its document and
edition.

The organising idea
-------------------

ISO 14229 is not one standard but a series, and it is already layered. The stack follows
that series with one crate per document, so "does this belong here?" is answered by
asking which document specifies the behaviour. ISO 14229-1 is the one exception: it is
split into its wire format, ``uds_protocol``, and its application behaviour,
``uds_services``.

The result is reuse. A session layer that reads no clock and calls no transport serves
CAN, DoIP, K-line and simulation alike; a service dispatcher that knows how to fetch a
data identifier has nothing to say about IP. Only the crates that name a transport are
bound to one.

.. uml::
   :align: center
   :caption: The stack today. Solid edges are calls; dashed edges are the message
             vocabulary, which ``uds_services`` and ``uds_on_ip`` read messages with.

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
   package "C firmware" {
     [uds_c] as UC
   }

   CLI -down-> SVC : typed requests
   APP -down-> SVC : typed service handlers
   UC -down-> SVC : table-driven handlers
   UC -down-> DOIP : sockets over the C ABI
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
     - The session layer: the ``S_Data`` primitives and the ``tP_Client``,
       ``tP2_Server``, ``tP3`` and ``tS3`` timers. It is sans-io: it takes a
       caller-supplied timestamp and returns actions to drain.
   * - ``uds_services``
     - ISO 14229-1:2020, everything but the message format
     - The point where an application meets the stack: per-service handler traits, the
       dispatch and negative-response rules of ISO 14229-1:2020 clause 8.7, the typed
       client calls and the client's timing policy, and the driver that holds the
       session layer and calls a transport.
   * - ``uds_on_ip``
     - ISO 14229-5:2022
     - The UDSonIP binding: the ``T_Data`` to ``DoIP_Data`` mapping, the ``tP6`` reload
       pair the session layer runs on at the least, and the profile's connection handling
       around a session change or a reset.
   * - ``simple_doip``
     - ISO 13400-2:2019
     - DoIP: framing, the TCP and UDP sockets, routing activation, alive check, vehicle
       identification and the ``DoIP_Data`` primitives.
   * - ``uds_c``
     - none: the C ABI of ``include/uds_c.h``
     - A server for firmware written in C: an application serving every staged service
       from layers of C tables and handlers, the record and operation-status protocols
       those handlers answer through, ``edge-nal`` sockets over the firmware's socket
       calls, and a clock over its millisecond counter.

Each also has something it must not acquire, and that is the more useful half:

- ``uds_protocol`` stays a codec: no dispatch, no policy, no session state.
- ``uds_session`` never learns which transport it runs over, and never reads a clock.
  ISO 14229-2:2021 9.1.2 splits ``tP_Client``'s reload parameters four ways by whether the
  transport offers ``T_DataSOM.ind``; the session layer holds a *default* and an
  *enhanced* pair, and its caller decides which values those are.
- ``uds_services`` declares the transport seam and implements no transport. Nothing on that
  seam is DoIP-shaped, so a CAN binding implements the same methods.
- ``uds_on_ip`` never learns what a service is. It reads three things of a message: whether
  a positive response answers ``DiagnosticSessionControl`` or ``ECUReset``, a request's
  suppressPosRspMsgIndicationBit, and the response-pending negative response. Those are
  ISO 14229-5:2022 clause 8 and transport rules keyed on a service, not service semantics.
- ``simple_doip`` never learns what a UDS message means. A diagnostic message's payload is
  an opaque byte string to it.
- ``uds_c`` never implements a standard. Every rule it serves by is a protocol crate's: it
  states the firmware's tables through the service traits of ``uds_services`` and carries
  the firmware's sockets under ``simple_doip``'s entity, and decides only what the C
  contract leaves to it.

``automotive-wire-codec``, outside this repository, provides the zero-copy ``no_std`` codec
traits ``uds_protocol``, ``simple_doip`` and ``uds_services`` are built on.

The six crates release in lockstep to crates.io: one version for the stack, published
bottom-up in dependency order.

When and what
-------------

Several questions about where a behaviour belongs have the same answer:

   **ISO 14229-5 decides *when*. ISO 13400-2 performs *what*.**

- **Reconnect and repeat routing activation** (ISO 14229-5:2022 REQ 7.8, REQ 7.10): before
  diagnostic communication continues, ``uds_on_ip`` sequences it and ``simple_doip``
  performs it. The routing activation request is ISO 13400-2's, and a tester sends it, so
  only a tester reconnects.
- **Close after a positive response** (ISO 14229-5:2022 REQ 7.9, REQ 7.11): ``uds_on_ip``
  decides and ``simple_doip`` closes. Closing a TCP connection is an ISO 13400-2 mechanism;
  the rule that a close follows *this particular* response is ISO 14229-5's. REQ 7.11's
  close follows every positive ``ECUReset`` response; REQ 7.9's follows a positive
  ``DiagnosticSessionControl`` response only where the server leaves the software it is
  running. That fact is the server's own, so ``uds_services`` states it and ``uds_on_ip``
  decides what follows.
- **Periodic responses** (ISO 14229-5:2022 REQ 7.7, REQ 7.16): payload type ``0x8004``,
  which ISO 13400-2:2019 does not define, so ``uds_on_ip`` interprets it and
  ``simple_doip`` carries it unread. A tester built on ``uds_on_ip`` receives one. A server
  cannot yet send one: neither ``UdsTransport`` nor ``DiagnosticEntity`` lets a sender
  choose a payload type.

The rule also says what must not happen. ``uds_services`` never decides when a connection
closes, because knowing that ISO 14229-5:2022 REQ 7.9 or REQ 7.11 requires a close is
ISO 14229-5 knowledge. ``simple_doip`` never recognises a service identifier to decide for
itself. A crate that must know *whether* to act has acquired the requirement, whether or
not it also performs it.

The seams
---------

A seam is declared by the crate that calls through it, so the implementor depends on the
declarer and a caller never depends on its callee's choice of binding. The connection seam
is the one exception.

**Transport seam: ``uds_services`` to a binding.** ``uds_services`` declares
``UdsTransport`` and ``TransportEvent``; ``uds_on_ip`` implements them over DoIP. An event
borrows the driver's buffer, never the transport, so a server can answer the request it
has just received. ``ClientTransport`` adds ``close`` for the client role alone; a server
is reconnected to and never reconnects, so the seam carries no ``reconnect``.

**Connection seam: ``uds_on_ip`` to ``simple_doip``.** The sockets are ISO 13400-2's, so
``simple_doip::service`` declares this seam, with no I/O in it: ``DiagnosticEntity``, a
whole entity behind one event stream, for a server; for a client,
``DiagnosticConnection``, one connection as a tester uses it, and ``TesterConnection``,
one it can also replace (``reconnect``) and end (``close``). This seam is declared by the
callee, because ``uds_on_ip`` already consumes ``simple_doip``'s addressing and the edge
cannot run the other way.

**``uds_session`` is vocabulary, not a seam.** ``uds_on_ip`` names ``uds_session``'s
addressing, ``SResult``, ``Timestamp`` and ``Reloads``, and calls none of its behaviour.
``uds_services`` holds the session layer by value, supplies its inputs and drains its
actions.

The seams in detail, and the design of the two bindings, are in each crate's architecture.

Timing ownership
----------------

Every timer has one owner, the crate that runs it. Where its value is policy rather than
a fixed parameter, the table also says who sets it.

.. list-table::
   :header-rows: 1
   :widths: 26 16 58

   * - Timer
     - Owner
     - Values, and notes
   * - ``tP_Client``
     - ``uds_session``
     - One per logical channel, physical and functional alike (ISO 14229-2:2021 REQ 5.26,
       Table 7). Starts on ``T_Data.conf`` and stops on ``T_Data.ind``
       (ISO 14229-2:2021 REQ 5.9, REQ 5.10), so over DoIP it starts on the entity's
       acknowledgement, not on the send. Its default and enhanced reloads start as the
       transport's pair: over DoIP, the ``tP6_Client_Max`` and ``tP6*_Client_Max`` the
       integrator gives ``uds_on_ip``, because DoIP has no ``T_DataSOM.ind``
       (ISO 14229-2:2021 REQ 5.11). After a positive ``DiagnosticSessionControl``
       response, ``uds_services`` raises them for that server to the ``P2Server_max`` and
       ``P2*Server_max`` it advertised plus ``ClientTiming::network_delay``, never below
       the transport's pair (ISO 14229-2:2021 Table 4).
   * - ``tP3_Client_Phys``, ``tP3_Client_Func``
     - ``uds_session``
     - One per physical and per functional channel (ISO 14229-2:2021 REQ 5.26, Table 7).
       The minimum spacing before the next request when none is required.
       ``uds_services`` sets the values, from ``ClientTiming::physical_spacing`` and
       ``functional_spacing``.
   * - ``tS3_Client``, ``tS3_Server``
     - ``uds_session``
     - One ``tS3_Server`` per server; a client needs one per point-to-point
       communication (ISO 14229-2:2021 REQ 5.26, Table 8).
   * - ``tP2_Server``, ``tP2*_Server``
     - ``uds_session``
     - Session timers, kept with the others rather than with the service that is slow.
       The session layer reports their expiry and does not answer it; see below.
   * - ``tP4_Server``
     - ``uds_services``, not as a timer
     - ISO 14229-2:2021 types it a performance requirement on the server's application,
       not a timer to run.
   * - Request repeats
     - ``uds_services``
     - ISO 14229-2:2021 9.7 Table 9 has the client repeat a request whose response window
       expired or whose transmission failed, at most twice. ``uds_session`` refuses a
       third repeat (``UDSS_LLR_0177``); ``uds_services`` performs the repeats, under the
       ceilings ``ClientTiming::repeats`` and ``ClientTiming::transmission_repeats``.
   * - Response-pending extensions
     - ``uds_services``
     - ``ClientTiming::max_response_pending`` caps how many response-pending messages one
       transmission of a physically addressed request accepts. ISO 14229-2:2021 leaves
       the number to the server, and the default accepts every one.
   * - ``A_DoIP_Diagnostic_Message``
     - ``simple_doip``
     - How long a tester waits for a request's acknowledgement (ISO 13400-2:2019
       Table 12), set with ``Tester::with_ack_timeout``. Its expiry fails the request's
       ``T_Data.conf``, which is the failed transmission ``transmission_repeats`` counts.
   * - Connect and routing activation
     - ``simple_doip``
     - ``ConnectOptions::timeout`` bounds a tester's connect and routing activation
       together. A reconnect also stops at the deadline of the ``uds_services`` call it
       serves, which ``uds_on_ip`` passes to ``TesterConnection::reconnect``.
   * - DoIP connection and announcement timers
     - ``simple_doip``
     - ``T_TCP_Initial_Inactivity``, ``T_TCP_General_Inactivity``, ``T_TCP_Alive_Check``,
       ``A_DoIP_Ctrl`` and the ``A_DoIP_Announce`` timers (ISO 13400-2:2019 Table 12).
       Transport-level, with no UDS meaning.

**Who sends response-pending.** A server that will not answer within ``tP2_Server`` sends a
response-pending negative response before it expires, which gives it ``tP2*_Server``.
``uds_session`` runs the timer and reports the coming expiry as an overrun, ahead of it by
the lead of ``UDSS_LLR_0186``; ISO 14229-2:2021 states no session layer action, so the
session layer sends nothing itself. ``uds_services`` runs the handler alongside the event
loop, so the report arrives while the handler works; it decides whether a
response-pending is admissible, builds it and submits it through the session layer.
``uds_on_ip`` sends it like any other ``T_Data.req``.

Standards and crates
--------------------

The standards' layers and the crate boundaries do not line up one-to-one:

- ISO 14229-1:2020 is two crates: ``uds_protocol`` is its wire format, and
  ``uds_services`` everything else in it.
- ISO 14229-5:2022 specifies an application profile above the session layer and a
  transport mapping below it. ``uds_on_ip`` implements both, and sits wholly below the
  session layer in the call order.
- The session layer's service user is the ISO 14229-1 layer, which is ``uds_services``;
  that crate is also the driver that supplies the session layer's time and drains its
  outputs.

The Cargo graph follows the seams rather than either picture. ``uds_on_ip`` depends on
``uds_services``, whose seam it implements, while ``uds_services`` calls it, and no crate
names a binding. The graph therefore has no cycle and needs no feature gate to avoid one.

.. uml::
   :align: center
   :caption: Cargo dependencies. An arrow points from a crate to a crate that depends on
             it, so left to right is publication order. Every edge is unconditional, and
             ``uds_c`` is the one leaf.

   @startuml
   left to right direction
   [automotive-wire-codec] as AWC
   [uds_protocol] as UP
   [simple_doip] as SD
   [uds_session] as US
   [uds_services] as USVC
   [uds_on_ip] as UOI
   [uds_c] as UC

   AWC --> UP
   AWC --> SD
   AWC --> USVC
   UP --> USVC
   UP --> UOI
   US --> USVC
   US --> UOI
   SD --> UOI
   USVC --> UOI
   SD --> UC
   USVC --> UC
   UOI --> UC
   @enduml

What a transport swap replaces
------------------------------

A binding is not a layer swapped within the stack. It is a complete path from ``A_Data``
to the wire, attached below the shared driver, so swapping replaces the binding together
with its transport while the driver, session, message and service crates stay. For
application code the swap point is the ``transport =`` parameter of ``uds_services``'
``uds_server!`` and ``uds_client!`` macros.

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
   * - ``uds_c``
     - its README and crate documentation, and ``include/uds_c.h``
