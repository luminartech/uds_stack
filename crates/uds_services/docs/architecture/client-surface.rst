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

Because nothing else in the stack offers it. ``uds_on_ip``'s client sends and receives
**bytes**:

.. code-block:: rust

   // uds_on_ip
   pub async fn send(&mut self, ta: Address, request: &[u8])
       -> Result<Completion<'_>, S::Error>;

and its documentation hands the rest upward in as many words: *"A UDS negative response is
not an error: it is a response, and interpreting it belongs to a higher layer."* This crate
is that higher layer. Without it, a client application assembles request bytes by hand and
parses responses by hand, which is the same failure the server side exists to prevent, on
the other side of the wire.

.. uml::
   :align: center
   :caption: A typed client request. Steps 2 and 7 are what this crate adds; everything
             between them is the binding's.

   @startuml
   autonumber "<b>[0]"
   hide footbox

   participant "client\napplication" as APP
   participant "uds_services" as V
   participant "binding\n(uds_on_ip)" as I
   participant server as S

   APP -> V : read(MyDid::VehicleSpeed)
   activate V
   V -> V : build request bytes\n(uds_protocol)
   V -> I : send(ta, bytes)
   activate I
   I -> S : request
   S --> I : response bytes
   I --> V : Completion::Responded(Indication)
   deactivate I
   V -> V : classify: positive, negative,\nor no response expected
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
   request with ``uds_protocol`` and decodes the response. The client application never
   assembles or parses request bytes.

   Clause 7.3.2's service request primitive is the operation being provided. This crate
   realises it as a call the client application makes, with the transport-facing half
   delegated to the binding.

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

   This element exists because the layer below explicitly refuses the job.
   ``uds_on_ip``'s client returns a ``Completion`` whose result is ``Ok`` for a negative
   response — correctly, since the exchange completed — and its documentation states that
   interpreting it belongs higher up. If this crate does not do it, every client
   application does it, differently.

   Symmetry with the server side is the point: ``UDSSVC_ARCH_0010`` writes the three bytes
   of a negative response, and this element reads them. Both refer to the same clause.

.. arch:: A functionally addressed request yields a lending sequence
   :id: UDSSVC_ARCH_0022
   :depends_on: UDSSVC_ARCH_0020
   :status: draft
   :origin: derived
   :tags: client; addressing

   A functionally addressed request is typed as a sequence of zero or more responses, each
   carrying the source address of the server that produced it. It is not typed as a single
   result, and the sequence is *lending*: a response borrows the receive buffer and is
   valid only until the next is taken.

   Rationale: a functional request reaches every server on the bus, so zero or more may
   answer — and by ``UDSSVC_ARCH_0009``, the servers that do not support it answer with
   silence. There is no single response to return, and typing it as one would force the
   API to lie about the common case.

   The source address is what makes the sequence usable. ``uds_on_ip``'s indication
   records that for a functionally addressed request each responding server sets its own
   source address, and that this is how responses are told apart. A sequence of decoded
   responses without their senders would be unattributable.

   Note that this is the exact inverse of the server-side decision in
   ``UDSSVC_ARCH_0015``, which excludes peer identity from the request context. Both are
   right. Clause 8.7's server rules read no address, so carrying one would be dead weight;
   a client with several answers to one question can do nothing with them unless it knows
   who sent each.

   Lending rather than owning follows from ``UDSSVC_ARCH_0017``: an owned sequence
   allocates per response, which is what the whole crate is shaped to avoid. It is the
   same reasoning that keeps ``uds_on_ip``'s equivalent from being a ``Stream``.

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

   This mirrors ``Outcome::Suppress`` on the server side (``UDSSVC_ARCH_0016``), and it is
   the same distinction the binding already draws — its completion type separates a
   response arriving from a request being transmitted with no response expected.

.. arch:: One identifier vocabulary serves both roles
   :id: UDSSVC_ARCH_0024
   :depends_on: UDSSVC_ARCH_0014
   :status: draft
   :origin: derived
   :tags: client; identifiers

   The identifier types an application defines are the same types used to implement its
   server handlers and to issue its client requests. There is not a server-side identifier
   type and a client-side one.

   Rationale: this is the strongest argument for the application owning its identifier
   enumerations, and it was missing from ``UDSSVC_ARCH_0014``, which justified them on
   completeness-checking alone. A vehicle programme writes one identifier catalogue and
   builds both an ECU and a tester against it. Two types for one catalogue means two
   places to add an identifier and no way for the compiler to notice when only one of them
   was updated.

   The consequence for the API is a real constraint rather than a nicety: an identifier
   type cannot be an associated type of a *server* trait alone, because client code that
   implements no server trait must still be able to name it. Where it is declared instead
   is unsettled — see :doc:`open-questions`.

.. needflow::
   :filter: "client" in tags
   :link_types: depends_on
   :align: center
