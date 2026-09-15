The seams
=========

What crosses each boundary of this crate, and who owns what on either side.

Six seams, in two groups.

Four are declared elsewhere and implemented here, because this crate is *called* across
them: the context supplied with a request, the outcome reported back, the sink a response
is written into, and the adapter onto a binding's byte interface. A server must be told
the state its decisions depend on, and the caller's shape is what it is told in.

Two are declared here and implemented elsewhere, because this crate *calls* across them:
the client's transport seam, since a client initiates rather than waits, and the
response-pending seam, since a server that has decided to say "still working" must reach a
transport it does not own.

**Whoever is called declares the interface.** That single rule produces all six and
explains why they point in opposite directions.

.. uml::
   :align: center
   :caption: The four seams declared elsewhere and implemented here. The two this
             crate declares are below.

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
   SESS -up-> DRV : session, security
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

   Rationale: taking these as parameters rather than through a dependency is what makes the crate
   usable under any binding, and avoids depending on ``uds_session``, which is private. The
   pipeline becomes a function of the request and this context, so every clause 8.7 rule is
   testable without a network, a clock or a session layer.

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

   **Whether a response-pending has been sent is deliberately not a field here, and an
   earlier draft had it wrong in a way worth recording.** Clause 8.7.5 guards both
   suppression rules on it, so the pipeline is incorrect without the fact — but it is not an
   *input*. ``UDSSVC_ARCH_0032`` makes this crate the originator of a 0x78, so the pipeline
   knows by construction. Carrying it on ``Ctx`` would have been wrong even had the
   originator been elsewhere: ``Ctx`` is snapshotted when a request arrives, and a
   response-pending is emitted *after* that snapshot and before the suppression gate reads
   it, so a by-value field would be stale at the only instant it is consulted.

   Peer identity is deliberately absent. An application that keys state per tester — a
   security-access attempt counter, say — holds that state in its own server type, where it
   already holds everything else.

   The client side answers this question the other way, and both answers are right.
   ``UDSSVC_ARCH_0022`` requires a client's functional responses to carry each responder's
   source address, because a client with several answers to one question cannot use them
   without knowing who sent each. A server has one request in front of it and no clause
   8.7 decision that reads an address.

   One challenge to this element is worth recording rather than settling here. ISO
   14229-1 clause 7.4.1 makes ``A_SA``, ``A_TA`` and ``A_TA_Type`` *mandatory* parameters
   of every application layer service primitive; this element carries only the third. The
   defence is that clause 8.7 reads only the third and ``UDSSVC_ARCH_0001`` bounds the
   crate to clause 8.7 — but it is a defence, not an absence of tension. See
   :doc:`open-questions`.

Outcome
-------

.. arch:: Dispatch is asynchronous and reports an outcome; only sink failures are errors
   :id: UDSSVC_ARCH_0016
   :depends_on: UDSSVC_ARCH_0009; UDSSVC_ARCH_0030
   :status: draft
   :origin: derived
   :tags: seam; outcome; async

   Rationale: a negative response is a normal, specified outcome of clause 8.7 expressed in the
   written bytes — a server answering 0x11 has succeeded at its job. Typing it as an error
   would put the most common non-trivial path through the crate into the ``Err`` branch,
   and callers would learn to ignore errors. Dispatch instead reports whether a response
   was written and should be transmitted, or whether nothing is to be sent. A negative
   response is the first of those, not an error. The error type is the sink's own.

   Silence must be a distinguishable outcome rather than "wrote nothing", because the
   caller has to tell "clause 8.7 requires no response" apart from "the handler produced an
   empty response" apart from "something went wrong". Only the first is a reason not to
   transmit.

   Genuine transport failures are the binding's concern and never reach a handler.

   **Dispatch and the handlers it calls are asynchronous**, which is what
   ``UDSSVC_ARCH_0030``'s assumption is spent on here. The gain is specific: a handler
   that takes longer than ``tP2_Server`` yields at its await points, so dispatch can offer
   a ``0x78`` across ``UDSSVC_ARCH_0031``'s seam while the handler it covers is still
   running, and the binding's driver keeps draining session actions throughout.

   Without this, that is machinery every integrator has to build — the driver must arrange
   for a slow handler to run somewhere it can continue past, and nothing in this crate or
   the binding does it for them. See :doc:`not-owned`.

   The honest limit: an asynchronous seam does not make blocking work non-blocking. A
   handler that busy-waits on a flash erase stalls the executor exactly as a synchronous
   one would, and the gain is real only for handlers written to yield. What changes is that
   yielding is now *possible* at the seam, where before it was not expressible at all.

Response sink
-------------

.. arch:: A response is written into a caller-supplied sink
   :id: UDSSVC_ARCH_0017
   :depends_on: UDSSVC_ARCH_0001
   :status: draft
   :origin: derived
   :tags: seam; no_std

   Rationale: allocation-freedom cannot be retrofitted, because the signatures that make an API
   alloc-free are the ones callers depend on: adding it later is a breaking change to every
   handler in every application. A handler therefore writes its response into an
   ``embedded_io::Write`` sink supplied by the caller, rather than returning an owned
   response. No public type carries a ``Vec`` or a ``String``, and the crate builds under
   ``no_std`` without ``alloc``.

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

   Rationale: this keeps the dependency edge pointing outward from this crate (``UDSSVC_ARCH_0002``)
   and keeps the binding ignorant of what a service is. The adapter is small by
   construction: if it ever needs to make a decision, that decision belongs on one side of
   the seam or the other, not in the conversion.

   The conversion is where a mismatch between this crate's context and a binding's becomes
   visible, which is its second purpose. Three mismatches stand today, and all three are
   changes to ``uds_on_ip`` rather than to this crate:

   * its context carries an addressing triple, the active session and the security level,
     but not the authentication state that ``UDSSVC_ARCH_0015`` requires;
   * its handler seam is synchronous, and ``UDSSVC_ARCH_0016`` requires an asynchronous
     one;
   * ``RequestHandler::handle`` must take ``UDSSVC_ARCH_0031``'s responder as a further
     parameter, since ``Ctx`` is ``Copy`` and cannot carry a ``&mut``.

   The last two are cheap now and expensive later: that trait is declared and has no
   implementations, so changing it costs a signature today and a migration once servers
   exist. This is the reason the stack is being iterated as a whole rather than crate by
   crate.

Client transport seam
---------------------

.. arch:: This crate declares the client transport seam; the binding implements it
   :id: UDSSVC_ARCH_0029
   :depends_on: UDSSVC_ARCH_0003; UDSSVC_ARCH_0030
   :status: draft
   :origin: derived
   :tags: seam; transport; client

   The asynchronous client of ``UDSSVC_ARCH_0028`` is generic over a transport trait
   declared by this crate. A binding implements it; behind that binding's feature this
   crate supplies the implementation for the binding's own client.

   .. code-block:: rust

      pub trait UdsTransport {
          type Error;

          /// Send a physically addressed request and await its reply.
          async fn send(&mut self, request: &[u8])
              -> Result<Reply<'_>, Self::Error>;

          /// Send a functionally addressed request and iterate the replies.
          async fn send_functional(&mut self, request: &[u8])
              -> Result<Replies<'_, Self>, Self::Error>;
      }

   Rationale: this is the exact inverse of ``UDSSVC_ARCH_0018``, and the asymmetry is deliberate
   rather than an inconsistency. The **byte seam is declared by the binding**,
   because the binding calls into a server and the shape of that call is transport-shaped.
   The **transport seam is declared here**, because this crate calls out to a transport and
   the shape of *that* call is service-shaped: it must distinguish a physically addressed
   request answered once from a functionally addressed request answered zero or more
   times, which is a clause 8.7 distinction and not a DoIP one.

   Two consequences. The trait's reply types are **this crate's own**, not the binding's —
   the adapter converts, exactly as it does for ``Ctx`` under ``UDSSVC_ARCH_0018`` — so the
   typed client follows to CAN unchanged. And this crate depends on no executor: the trait
   is asynchronous, and the runtime is the caller's (``UDSSVC_ARCH_0030``).

   The functional case is why the trait carries two methods rather than one with a flag. A
   functionally addressed request yields a lending sequence whose length is not known until
   a timer expires (``UDSSVC_ARCH_0022``); typing it as the same operation as a
   single-response exchange would force one of the two to lie.

Response-pending seam
---------------------

.. arch:: This crate declares the response-pending seam; the driver implements it
   :id: UDSSVC_ARCH_0031
   :depends_on: UDSSVC_ARCH_0016; UDSSVC_ARCH_0030; UDSSVC_ARCH_0032
   :status: draft
   :origin: derived
   :tags: seam; transport; async; response-pending

   A server that has decided to answer ``requestCorrectlyReceivedResponsePending`` (0x78)
   while a handler is still running must reach a transport it does not own, and must be
   told when the moment to do so has arrived. Both cross one seam, declared here and
   implemented by the binding's server driver:

   .. code-block:: rust

      pub trait PendingResponder {
          /// Resolves when a response-pending should next be considered.
          ///
          /// Cancel-safe: dispatch drops and recreates this future every time
          /// the handler makes progress.
          async fn due(&mut self);

          /// Submit a response-pending for transmission. Returns immediately.
          /// `false` means the session layer refused it and nothing was sent.
          fn offer(&mut self, message: [u8; 3]) -> bool;
      }

   Rationale: ``UDSSVC_ARCH_0032`` puts the decision here, and a decision that cannot be
   acted on is not a decision. The seam is declared here rather than by the binding
   because this crate calls across it, which is the same rule that places
   ``UDSSVC_ARCH_0029``'s transport seam here and ``UDSSVC_ARCH_0018``'s byte seam on the
   other side.

   Dispatch races the handler against ``due`` and offers while it waits:

   .. code-block:: rust

      let mut handler = pin!(self.handle(..));
      let mut sent = false;
      let settled = loop {
          select! {
              settled = &mut handler => break settled,
              () = pending.due() => if MAY_RESPOND_PENDING {
                  sent |= pending.offer([0x7F, sid, 0x78]);
              }
          }
      };

   Four properties, each of which was arrived at rather than assumed.

   **The message is passed by value, in three bytes.** ISO 14229-1:2020 Annex A gives
   ``requestCorrectlyReceivedResponsePending`` no payload, so a response-pending is always
   ``0x7F``, the echoed service identifier, and ``0x78``. Passing it by value removes the
   second sink, the borrow against the one ``UDSSVC_ARCH_0017`` already holds, and any
   allocation question. Composing it here rather than in the driver is what keeps
   ``UDSSVC_ARCH_0019`` true: the driver transmits bytes it need not understand.

   **``offer`` returns immediately and reports nothing.** Submission and confirmation are
   two moments in ISO 14229-2:2021, not one — ``UDSS_LLR_0145`` stops ``tP2_Server`` at the
   ``T_Data.req`` and ``UDSS_LLR_0218`` anchors the spacing at the ``T_Data.conf`` — and
   awaiting the second inside dispatch would stall the handler for the duration of a
   transmission, which is the opposite of what ``UDSSVC_ARCH_0016``'s asynchronous seam
   exists to buy.

   **``offer`` reports refusal but not failure, and the asymmetry is the point.** A
   submission the session layer refuses — for spacing under ``UDSS_LLR_0285``, for an
   unconfirmed predecessor under ``UDSS_LLR_0284``, or for an association that is duplicated
   or exhausted under ``UDSS_LLR_0273`` and ``UDSS_LLR_0274`` — is rejected *synchronously*,
   at the moment it is supplied, and the driver knows at once. That is a clause 8.7 fact:
   nothing was sent, so ``UDSSVC_ARCH_0009``'s override must not fire. A transmission that
   is accepted and later fails is a different thing, reported asynchronously as a
   ``T_Data.conf``, and is the binding's to absorb under ``UDSSVC_ARCH_0016``. So the return
   is a ``bool`` rather than a ``Result``: it answers "was this accepted for transmission",
   which the driver can pass straight through, and never "did it arrive", which would oblige
   the driver to track confirmations on this crate's behalf.

   **``due`` must be cancel-safe, and that is a contract rather than a note.** The future
   is dropped and rebuilt on every handler wakeup. A driver that implemented it by starting
   a fresh interval on each call would never become due under a handler that yields often,
   and the failure would appear only under load.

   A server whose services all decline response-pending under ``UDSSVC_ARCH_0033`` still
   names a responder. This crate supplies an implementation whose ``due`` never resolves,
   which costs nothing at runtime; making the parameter optional would double the dispatch
   signature to avoid a type that is already free.

   The seam adds a fourth item to the changes ``UDSSVC_ARCH_0018`` requires of
   ``uds_on_ip``: ``RequestHandler::handle`` gains the responder as a parameter, since
   ``Ctx`` is ``Copy`` and cannot carry a ``&mut``.
