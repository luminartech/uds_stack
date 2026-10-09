The client surface
==================

The other half of the integration point: what a client application can ask for, and what
it gets back.

ISO 14229-1 clause 7 specifies the application layer service access point symmetrically.
Six primitives per service: ``.req``, ``.req-conf`` and ``.conf`` used by the client
function in the tester application, and ``.ind``, ``.rsp`` and ``.rsp-conf`` used by the
server function in the ECU application. Clause 7.1 is explicit that these services are
client-server, and that the client uses them to *request* diagnostic functions.

The crate's normative scope remains clause 8.7 — the server response implementation rules
are what it implements as a standard behaviour. The client surface is not clause 8.7, and
the elements below say so: each cites the clause that actually shapes it where one exists,
and is ``derived`` where the shape is ours.

Why it is here at all
---------------------

Because nothing else in the stack offers it. ``uds_session``'s client role tracks channels,
response windows, spacing and keep-alive, and indicates each response as a message;
``UdsTransport`` carries bytes. Neither knows a service or an identifier, and interpreting
a negative response belongs to the layer above both. This crate is that layer. Without it,
a client application assembles request bytes by hand and parses responses by hand, which
is the same failure the server side exists to prevent, on the other side of the wire.

.. uml::
   :align: center
   :caption: A typed client request. Steps 2 and 9 are the encoding and the
             interpretation; between them this crate's driver runs the exchange through
             uds_session over the transport.

   @startuml
   autonumber "<b>[0]"
   hide footbox

   participant "client\napplication" as APP
   participant "uds_services" as V
   participant "uds_session\nclient role" as SES
   participant "UdsTransport" as T
   participant server as S

   APP -> V : read_data_by_identifier(ta, [MyDid::VehicleSpeed])
   activate V
   V -> V : encode request bytes
   V -> SES : s_data_req
   SES --> V : Transmit
   V -> T : t_data_req
   T -> S : request
   T --> V : DataConf, then DataInd(response)
   V -> SES : t_data_conf, t_data_ind (classified)
   V -> V : interpret: positive, negative,\nor malformed
   V --> APP : typed result over MyDid
   deactivate V
   @enduml

The elements
------------

.. arch:: A client issues requests over the application's own identifiers
   :id: UDSSVC_ARCH_0020
   :depends_on: UDSSVC_ARCH_0019; UDSSVC_ARCH_0024
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 7.1; ISO 14229-1:2020 7.3.2
   :tags: client; api

   A client application names a service and its parameters in the application's own
   vocabulary — the identifier types of ``UDSSVC_ARCH_0024`` — and this crate encodes the
   request and decodes the response, with ``uds_protocol``'s service identifiers and
   negative response codes. The client application never assembles or parses request
   bytes.

   The bytes between the service identifier and the end are the application's identifiers,
   written big-endian, and a positive response is walked against the same identifiers. Every
   ``uds_protocol`` request type is built either from a list the client does not hold or
   from the wire bytes the client would already have written, so routing the encoding
   through one would add a copy and no check.

   Clause 7.3.2's service request primitive is the operation being provided. This crate
   realises it as a call the client application makes, exchanged over ``UdsTransport``
   (``UDSSVC_ARCH_0029``) by this crate's own driver.

   The asymmetry with the server side is deliberate and worth stating, because it looks
   like an inconsistency. A server *implements* a trait per service and is called; a client
   *calls* a function per service and implements nothing. Both directions are the same
   clause 7 access point seen from opposite ends, and neither shape fits the other end:
   there is nothing for a client to implement, and nothing for a server to call.

.. arch:: A negative response is a response, and this crate interprets it
   :id: UDSSVC_ARCH_0021
   :depends_on: UDSSVC_ARCH_0020
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.6
   :tags: client; nrc

   A negative response received by a client is decoded into the service it answers and its
   negative response code, and reported as a normal outcome of the request. It is not an
   error, and it is not raw bytes.

   Clause 8.6 defines the negative response/confirmation service primitive, which is what
   makes this an interpretation with a definition behind it rather than a convention.

   This element exists because nothing below does the job. ``uds_session`` indicates a
   response as a message, classified only as final or pending, and ``UdsTransport`` carries
   bytes; interpreting a negative response belongs to the application layer. If this crate
   does not do it, every client application does it, differently.

   Symmetry with the server side is the point: ``UDSSVC_ARCH_0010`` writes the three bytes
   of a negative response, and this element reads them. Both refer to the same clause.

.. arch:: A functionally addressed request yields a lending sequence
   :id: UDSSVC_ARCH_0022
   :depends_on: UDSSVC_ARCH_0020
   :status: draft
   :origin: derived
   :tags: client; addressing

   Rationale: a functional request reaches every server on the bus, so zero or more may
   answer — and by ``UDSSVC_ARCH_0009``, the servers that do not support it answer with
   silence. There is no single response to return, so typing one would force the API to lie
   about the common case. A functionally addressed request is instead typed as a sequence of
   zero or more responses, each carrying the source address of the server that produced it,
   and the sequence is *lending*: a response borrows the receive buffer and is valid only
   until the next is taken.

   The source address is what makes the sequence usable. For a functionally addressed
   request each responding server sets its own source address, and ``UdsTransport``'s
   indication carries it: that is how responses are told apart. A sequence of decoded
   responses without their senders would be unattributable.

   Note that this is the exact inverse of the server-side decision in
   ``UDSSVC_ARCH_0015``, which excludes peer identity from the request context. Both are
   right. Clause 8.7's server rules read no address, so carrying one would be dead weight;
   a client with several answers to one question can do nothing with them unless it knows
   who sent each.

   Lending rather than owning follows from ``UDSSVC_ARCH_0017``: an owned sequence
   allocates per response, which is what the whole crate is shaped to avoid. It is also why
   the sequence is not a ``Stream``: a ``Stream`` item cannot borrow the receive buffer.

   The sequence closes when the functional channel's response window does: one response
   timeout after the last answer, a response-pending message holding it open meanwhile.
   Nothing is sent until the first answer is asked for. The request in progress is recorded
   in the client rather than in the sequence, so a sequence dropped part-way is drained by
   the client's next call, and no answer arriving inside its window is taken for the next
   request's. One arriving after the window closed is unsolicited unless it echoes the next
   request's service, or for a session change its session; the requirements open question
   "Should a reset discard a message already arriving?" records that limit.

.. arch:: No response expected is a normal client outcome
   :id: UDSSVC_ARCH_0023
   :depends_on: UDSSVC_ARCH_0020
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.2 Table 4; ISO 14229-1:2020 8.7.4.2 Table 6
   :tags: client; suppression

   A request sent with the suppress-positive-response bit set, and a functionally
   addressed request that no server supports, both complete without a response. The client
   surface reports that as an outcome of the request, distinct from a timeout and from an
   error.

   A client that cannot express "completed, nothing came back" has to represent it as a
   timeout, and a timeout is a fault. Suppression is not a fault: the client asked for it,
   or clause 8.7 required it.

   This mirrors ``Responded::Suppressed`` on the server side (``UDSSVC_ARCH_0016``). The
   faults are the client's ``ClientError``: a response window that expired after
   ISO 14229-2:2021 9.7 Table 9's repeats, a transmission that failed after them, and a
   connection that closed first.

.. arch:: One identifier vocabulary serves both roles
   :id: UDSSVC_ARCH_0024
   :depends_on: UDSSVC_ARCH_0014
   :status: draft
   :origin: derived
   :tags: client; identifiers

   Rationale: a vehicle programme writes one identifier catalogue and builds both an ECU and
   a tester against it. Two types for one catalogue means two places to add an identifier
   and no way for the compiler to notice when only one of them was updated. The identifier
   types an application defines are therefore the same types used to implement its server
   handlers and to issue its client requests: there is not a server-side identifier type and
   a client-side one.

   The consequence for the API is a real constraint rather than a nicety: an identifier
   type cannot be an associated type of a *server* trait alone, because client code that
   implements no server trait must still be able to name it. Where it is declared instead
   is unsettled — see :doc:`open-questions`.

Two layers
----------

.. arch:: The client core is sans-io; awaiting is a layer above it
   :id: UDSSVC_ARCH_0028
   :depends_on: UDSSVC_ARCH_0020; UDSSVC_ARCH_0029
   :status: draft
   :origin: derived
   :tags: client; api; no_std

   The client is two layers. The lower one is sans-io and synchronous: it encodes a typed
   request into a caller-supplied buffer, and interprets response bytes into a typed
   result. The upper one is the asynchronous client that joins the two halves across a
   transport, so an application writes one call and awaits it.

   .. code-block:: rust

      // core: no async, no transport, no_std (crate-internal: client::encode)
      let n = encode::read_data_by_identifier(&mut request, &[MyDid::VehicleSpeed])?;
      let value = encode::records::<MyDid>(encode::final_response(&response, Arrived::Whole));

      // layer: one call, over any transport
      let value = client.read_data_by_identifier(ta, &[MyDid::VehicleSpeed]).await?;

   The lower layer is not public. Its tests bind to it inside the crate, which is the use
   this element names; a caller that needs it without a transport is a reason to publish
   it, and none has appeared.

   Rationale: everything below this crate is asynchronous. ``UdsTransport`` is an
   ``async`` trait, and the bindings that implement it run over sockets. A typed client
   that did not await would therefore hand the application a buffer, make it call the
   binding, and hand back the bytes, which is precisely what ``UDSSVC_ARCH_0020`` says a
   client application never does.

   **Why the server needs no equivalent and the client does.** A server *responds*, so the
   awaiting belongs to the loop that read the request: ``UDSSVC_ARCH_0040``'s driver holds
   the handler future and drives it. ``UDSSVC_ARCH_0016`` makes that handler asynchronous
   for a reason of its own — a handler outrunning ``tP2_Server`` must yield so the loop can
   submit a ``0x78`` (``UDSSVC_ARCH_0031``) while it runs — but the server still needs no
   *layer* over it, because nothing is being joined. A client *initiates*, so something must
   await, and if it is not this crate it is the application. The asymmetry is in the
   direction of control, not in the design.

   An earlier version of this passage said the driver "invokes a synchronous handler, and
   the inversion costs nothing". That was the design before ``UDSSVC_ARCH_0016``, and it
   cost exactly what that element now spends: with a synchronous handler a slow one blocks
   the driver, and every integrator builds the escape machinery themselves.

   **Why the lower layer exists at all**, rather than an asynchronous client alone. It is
   what a test binds to: no transport, no executor, no timing. Every clause 8.7 rule the
   client implements — interpreting a negative response, classifying a suppressed one,
   attributing functional replies — is then checkable as a pure function of bytes in and a
   typed value out. And it keeps the crate's core uniformly sans-io across both roles, so
   the property that makes the server testable holds for the client too, rather than
   holding for half the crate.

   ``async`` implies neither a runtime nor ``std``. Asynchronous functions in traits are
   stable and the executor is the caller's, so the upper layer costs this crate no
   dependency — see ``UDSSVC_ARCH_0029``.

.. uml::
   :align: center
   :caption: The two layers. Everything below the dashed line is asynchronous already;
             the core above it is not, and does not need to be.

   @startuml
   allowmixing

   rectangle "client application" as APP

   package "uds_services" {
     rectangle "async client" as ASYNC
     rectangle "sans-io core\nencode request / interpret response" as CORE
     rectangle "driver loop + uds_session" as DRV
     interface "UdsTransport" as TR
   }

   package "binding" {
     rectangle "DoIP client\ntransport" as BIND
   }

   APP -down-> ASYNC : read(..).await
   ASYNC -down-> CORE : encode / interpret
   ASYNC -down-> DRV : exchange
   DRV -down-> TR
   BIND .up.|> TR : implements
   @enduml

The client and the server share the loop and the transport beneath them. That is why
``UDSSVC_ARCH_0029`` declares one trait rather than a client one and a server one: the bytes
and the socket are the same, and only what this crate does with them differs.

.. needflow::
   :filter: "client" in tags
   :link_types: depends_on
   :align: center

The programming process
-----------------------

.. arch:: Clause 17 is a client sequence, not a server obligation
   :id: UDSSVC_ARCH_0039
   :depends_on: UDSSVC_ARCH_0028; UDSSVC_ARCH_0036; UDSSVC_ARCH_0037; UDSSVC_ARCH_0038
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 17.1; ISO 14229-1:2020 17.2
   :tags: client; scope; state

   ISO 14229-1 clause 17 specifies the non-volatile server memory programming process. It
   is written from the client's side — "the programming process **the client** is required
   to follow" — and it is an orchestration of services, not a new protocol behaviour.

   That placement is checkable rather than a matter of reading: across 625 lines, clause 17
   binds the server directly three times. One is the step-type definition, that "the client
   and the server shall behave as specified" for standardized steps, which defers to the
   services those steps use. The other two are properties of an ECU rather than of a
   protocol — that memory "shall be erased when required by the memory technology", and that
   a server "shall be able to recover and be reprogrammed" after an interrupted programming
   attempt.

   **So the server side of clause 17 is already discharged.** Its steps are
   ``DiagnosticSessionControl``, ``SecurityAccess``, ``RequestDownload``, ``TransferData``,
   ``RequestTransferExit``, ``RoutineControl`` and ``ECUReset``, and a server that
   implements those correctly has done what clause 17 asks of it. Correctly is the load
   here, and it is where ``UDSSVC_ARCH_0036``, ``UDSSVC_ARCH_0037`` and
   ``UDSSVC_ARCH_0038`` do the work: the transfer sequence, the security sequence and the
   session-transition rules are exactly the parts of a programming sequence that go wrong.
   No further server-side state is implied by this clause.

   Rationale: recording where clause 17 lands costs nothing now and prevents the wrong
   thing being built later. ``UDSSVC_ARCH_0001`` first listed it as in scope and not built,
   described as spanning exchanges and needing server state the dispatch had nowhere to
   hold. That was the wrong reading: the spanning is the client's.

   **What remains is a client orchestration, and it is deferred deliberately.** Clause 17.1
   classifies its own steps as standardized, optional/recommended, or vehicle-manufacturer
   specific, and lets a programme choose a functionally or a physically oriented vehicle
   approach for them. Only the standardized steps are common across programmes, so only
   they are library-able; the rest is a vehicle programme's own sequence expressed over
   ``UDSSVC_ARCH_0028``'s client. Building the orchestration before there is a client that
   can execute one step of it would be designing against nothing.

   The "master execute" coordination of 17.1, where steps are functionally addressed to
   every node and their results reconciled, is the same problem ``UDSSVC_ARCH_0022`` already
   solves for a functionally addressed request with several responders. That is the piece to
   build on when this is taken up.
