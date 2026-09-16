The seams
=========

What crosses each boundary of this crate, and who owns what on either side.

Six seams, and **this crate declares none of them.**

**Each is declared by the crate that owns the document specifying it.** ``uds_session``
declares the handler seam, the outcome, the request context, the client seam and the
response-pending seam, because ISO 14229-2 specifies both interfaces of the session layer;
``automotive-wire-codec`` declares the sink, because the I/O vocabulary is its. This crate
implements two of them and calls three, which is a fact about the direction of control and
no longer a fact about who wrote the trait.

That single rule replaces the one this page used to give. **"Whoever is called declares the
interface"** produced a tidy symmetry — the byte seam declared by the binding because the
binding calls in, the client seam declared here because this crate calls out — and it was
wrong about the first half. A seam carrying an addressing triple, bytes, a sink and a
responder is not transport-shaped, so every binding would have declared the same trait
separately. Ownership follows the specifying document, not the direction of the call.

.. uml::
   :align: center
   :caption: The server-side seams. Every trait on this diagram is declared by
             ``uds_session``; the sink is ``automotive-wire-codec``'s.

   @startuml
   package "binding (uds_on_ip, uds_on_can)" {
     [server driver] as DRV
     [uds_session] as SESS
   }

   package "uds_services" {
     [dispatch pipeline] as PIPE
   }

   package "application" {
     [typed service handlers] as APPH
   }

   interface "request bytes" as BYTES
   interface "Ctx" as CTX
   interface "Outcome" as OUT
   interface "Write sink" as SINK

   DRV -down-> BYTES
   BYTES -down-> PIPE
   DRV -down-> CTX
   SESS -up-> DRV : addressing, timing
   CTX -down-> PIPE
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
   :filter: "seam" in tags
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
        - Status
      * - ``uds_session::Ai`` — the ISO 14229-2 addressing triple
        - Suppression (``UDSSVC_ARCH_0009`` rule 1) reads the target address type;
          ``UDSSVC_ARCH_0035`` keys per-channel state on the source address
        - Carried
      * - active session
        - Figure 5 → 0x7F, Figure 6 → 0x7E
        - Under review — see below
      * - security level
        - Figure 5 and Figure 6 optional checks → 0x33
        - Under review — see below
      * - authenticated
        - Figure 5 and Figure 6 → 0x34
        - Under review — see below

   Rationale: the pipeline is a function of the request and this context, so every clause 8.7
   rule is testable without a network, a clock or a session layer.

   **Three fields are under review, and this crate is why.** It implements
   ``DiagnosticSessionControl``, ``SecurityAccess`` and ``Authentication``, so under
   ``UDSSVC_ARCH_0035`` and ``UDSSVC_ARCH_0037`` it already holds the active session, the
   security level and the authentication state. Two crates tracking the same state is how
   they come to disagree. Unless an integration turns up where something else owns them,
   ``Ctx`` reduces to the addressing triple alone — at which point it is worth asking whether
   it stays a struct at all. To settle with ``uds_session`` before either crate writes it.

   **Session and security are raw sub-function values, passed through uninterpreted.**
   Naming the sessions here would mean this crate deciding what
   ``DiagnosticSessionControl``'s sub-functions mean, which is ``uds_protocol``'s to define
   and the application's to choose.

   **Addressing is ``uds_session::Ai``, the ISO 14229-2 triple, used directly.** An earlier
   version of this element defined a local two-variant physical/functional type instead, on
   the ground that a binding's addressing triple is transport-shaped — ``uds_on_ip``'s
   carried a DoIP logical address — so embedding one would make a transport a mandatory
   dependency of the typed layer. That objection was to the *binding's* type, and it no
   longer applies: the triple now lives in ``uds_session``, shaped by ISO 14229-2 clauses
   8.3–8.6 rather than by DoIP, and ``UDSSVC_ARCH_0002`` already depends on that crate.

   Using it directly also closes a question the set had been carrying about whether two
   vocabularies for one concept were worth the conversion. There is one.

   **Whether a response-pending has been sent is deliberately not a field here, and an
   earlier draft had it wrong in a way worth recording.** Clause 8.7.5 guards both
   suppression rules on it, so the pipeline is incorrect without the fact — but it is not an
   *input*. ``UDSSVC_ARCH_0032`` makes this crate the originator of a 0x78, so the pipeline
   knows by construction. Carrying it on ``Ctx`` would have been wrong even had the
   originator been elsewhere: ``Ctx`` is snapshotted when a request arrives, and a
   response-pending is emitted *after* that snapshot and before the suppression gate reads
   it, so a by-value field would be stale at the only instant it is consulted.

   **Peer identity comes with the triple, and it is needed.** ISO 14229-1:2020 10.6.4
   requires that "an authenticated state shall be linked to a certain diagnostic channel" and
   that "multiple clients can be handled on multiple channels with different authentication
   settings", so ``UDSSVC_ARCH_0035``'s per-channel state cannot be keyed without the source
   address. An earlier draft of this element excluded peer identity altogether and offered a
   security-access attempt counter as the example of state an application should key for
   itself — wrong twice over: clause 10.4 makes security access server-global, so it needs no
   key, and ``UDSSVC_ARCH_0034`` puts the counter in the stack rather than in the
   application.

   Taking ``Ai`` whole also settles ISO 14229-1 clause 7.4.1, which makes ``A_SA``, ``A_TA``
   and ``A_TA_Type`` mandatory parameters of every application layer service primitive. All
   three are present. The set argued about this for some time as though it were a question of
   what clause 8.7 reads; it was a question of which vocabulary the seam speaks, and it
   dissolved when the vocabulary moved to the crate that owns ISO 14229-2.

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
   ``automotive_wire_codec::Sink`` supplied by the caller, rather than returning an owned
   response. No public type carries a ``Vec`` or a ``String``, and the crate builds under
   ``no_std`` without ``alloc``.

   **The sink is ``awc``'s rather than ``embedded-io``'s, and the difference produces an
   NRC.** ``Sink`` carries ``remaining()``. A binding bounds the sink at the transport's
   advertised maximum response length, an over-long response fails at the write with the
   needed and available counts intact, and this crate turns that counted failure into
   ``responseTooLong`` (0x14). That settles a question the set had been carrying: the maximum
   response length is a transport property this crate does not know, but the negative
   response code is a clause 8.7 outcome that is nobody else's. ``embedded-io`` leaves this
   crate's dependency list entirely.

   The sink is a **generic parameter, not ``dyn``**. That costs object safety — there is no
   ``Box<dyn UdsServer>``, and a driver is generic over the handler type instead. For a
   crate that must build without ``alloc``, where boxing is unavailable anyway, it is the
   right trade, but it is a deliberate one and should be revisited if a dynamic service
   registry is ever wanted.

   One consequence to design against rather than discover: a sink can fail mid-response,
   after some bytes are already written. What a partially written response means at the
   handler seam — whether the binding must discard it, and whether this crate must avoid
   writing until it can complete — is not settled. ``awc``'s decision to expose ``write_all``
   rather than ``write`` removes the *partial write* case, but not the *failed part-way
   through* case. See :doc:`open-questions`.

   ``no_std`` freedom is verified on a bare-metal target, not by
   ``--no-default-features`` on a hosted one. Only a ``*-none`` target proves ``std`` has
   not crept back in through a dependency.

The handler seam
----------------

.. arch:: uds_session declares the handler seam; this crate implements it
   :id: UDSSVC_ARCH_0018
   :depends_on: UDSSVC_ARCH_0002; UDSSVC_ARCH_0015; UDSSVC_ARCH_0016
   :status: draft
   :origin: derived
   :tags: seam; transport

   A binding's driver hands a request to this crate across one seam, declared by
   ``uds_session`` as ``RequestHandler``. This crate implements it once, for any assembled
   typed server. There is no per-binding adapter and no transport feature.

   Rationale: ISO 14229-2 specifies both interfaces of the session layer — ``A_Data`` upward
   and ``T_Data`` downward — so the application-facing seam is that document's, and
   ``uds_session`` is the crate that owns it. One declaration then serves every binding, and
   neither this crate nor ``uds_on_ip`` needs to name the other.

   **An earlier version of this element held that a byte seam is "necessarily
   transport-shaped — it is where a binding hands off", so each binding declared its own and
   this crate implemented whichever an enabled feature selected.** The premise does not
   survive inspection of the signature: it carries an addressing triple, request bytes, a
   sink and a responder, and no transport concept whatever. Every binding needed an identical
   seam, and three of them would have declared it separately. What was transport-shaped was
   ``uds_on_ip``'s ``Ai``, and moving the addressing vocabulary to ``uds_session`` removed
   even that.

   What the relocation costs is a dependency this crate previously refused. ``uds_session``
   is public now, and ``UDSSVC_ARCH_0002`` records what survives of the reasoning that
   refused it.

Client seam
-----------

.. arch:: uds_session declares the client seam; this crate uses it
   :id: UDSSVC_ARCH_0029
   :depends_on: UDSSVC_ARCH_0002; UDSSVC_ARCH_0030
   :status: draft
   :origin: derived
   :tags: seam; transport; client

   The asynchronous client of ``UDSSVC_ARCH_0028`` is generic over ``uds_session``'s
   ``DiagnosticClient``. A binding's driver implements it; this crate calls it and
   implements nothing.

   Rationale: the same argument as ``UDSSVC_ARCH_0018``, applied to the other direction of
   control. The shape of the call is an ISO 14229-2 service primitive — an ``Ai``, a request,
   and either one completion or a sequence of them — so ISO 14229-2's crate declares it, and
   one declaration serves every binding.

   The functional case is why the trait carries two methods rather than one with a flag. A
   functionally addressed request yields a lending sequence whose length is not known until a
   timer expires (``UDSSVC_ARCH_0022``); typing it as the same operation as a single-response
   exchange would force one of the two to lie. That distinction survives the move, because it
   is a property of addressing rather than of a transport.

   **An earlier version of this element declared the trait here**, as ``UdsTransport``, and
   argued it was the exact inverse of ``UDSSVC_ARCH_0018`` — the byte seam declared by the
   binding because the binding calls in, the transport seam declared here because this crate
   calls out. That symmetry was real but rested on the premise ``UDSSVC_ARCH_0018`` has since
   lost. With both seams declared by the document that specifies them, the two are no longer
   inverses; they are the same rule applied twice.

   This crate still depends on no executor: the trait is asynchronous and the runtime is the
   caller's (``UDSSVC_ARCH_0030``).

Response-pending seam
---------------------

.. arch:: uds_session declares the response-pending seam; this crate decides what crosses it
   :id: UDSSVC_ARCH_0031
   :depends_on: UDSSVC_ARCH_0016; UDSSVC_ARCH_0030; UDSSVC_ARCH_0032
   :status: draft
   :origin: derived
   :tags: seam; transport; async; response-pending

   A server that has decided to answer ``requestCorrectlyReceivedResponsePending`` (0x78)
   while a handler is still running must reach a transport it does not own, and must be
   told when the moment to do so has arrived. Both cross one seam, ``PendingResponder``,
   declared by ``uds_session`` because *when* a response-pending is due is ``tP2_Server``
   timing, implemented by the binding's server driver, and called from here:

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
   acted on is not a decision. The declaration sits with ``uds_session`` for the same reason
   as ``UDSSVC_ARCH_0018`` and ``UDSSVC_ARCH_0029``: ISO 14229-2 owns the timing that says
   when one is due. It does not own whether one is owed, or the bytes — ISO 14229-2:2021
   REQ 5.6 makes admissibility turn on whether the server supports the service in progress,
   which is this crate's predicate and a session layer forbidden to inspect message data
   cannot evaluate.

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

   ``RequestHandler::handle`` takes the responder as a parameter rather than carrying it on
   ``Ctx``, which is ``Copy`` and cannot hold a ``&mut``.

Clock
-----

.. arch:: This crate declares the clock seam and ships one implementation
   :id: UDSSVC_ARCH_0041
   :depends_on: UDSSVC_ARCH_0030; UDSSVC_ARCH_0040
   :status: draft
   :origin: derived
   :tags: seam; async; driver; time

   Driving ``uds_session`` requires time: it is sans-io and evaluates a timer's expiry only
   when a timestamp is supplied. So ``UDSSVC_ARCH_0040``'s loop needs to know what time it
   is, and needs to wake when something is due.

   .. code-block:: rust

      pub trait Clock {
          /// Monotonic milliseconds, 32-bit and wrapping.
          fn now_ms(&self) -> u32;

          /// Resolve after at least `ms` have elapsed.
          async fn sleep(&mut self, ms: u32);
      }

   **The unit and width are ``uds_session``'s, deliberately.** ``UDSS_LLR_0114`` fixes a
   timestamp as a 32-bit count of milliseconds and specifies interval arithmetic modulo
   2\ :sup:`32`. Matching it exactly means no conversion sits between the clock and the
   state machine it feeds, and no second wraparound point exists to reason about.

   **``sleep`` takes a duration, and this is the part worth arguing.** A deadline in wrapping
   ``u32`` space is ambiguous on its own: ``5`` is either a moment just past or one roughly
   49 days away, and only a reference point separates them. This crate holds both values —
   the deadline ``uds_session`` reports and the ``now_ms`` it just read — so it computes the
   interval here, by the modular subtraction ``UDSS_LLR_0114`` already specifies, and passes
   a delay that cannot be misread. An implementor writing a ``sleep_until`` would have to
   rederive that reasoning, and every implementor would have to rederive it identically.

   **The deadline comes from ``uds_session``.** ``UDSS_LLR_0304`` has the session layer
   report the earliest timestamp at which supplying a timestamp could expire a timer, or
   report that none is running. That report and this trait are the two halves of one
   mechanism: the session layer says *when*, this crate computes *how long*, and the clock
   waits. Neither half is useful alone, and they were designed a day apart for unrelated
   reasons.

   Rationale: a clock is the one thing a driver cannot abstract over and cannot supply
   itself. Reading one directly would make every timing rule in the stack untestable except
   in real time, which is the argument ``UDSS_LLR_0114`` makes for the layer below; taking
   it as a seam keeps a test able to advance time by returning larger numbers.

   **One implementation ships, behind a feature.** A ``tokio``-backed ``Clock`` for hosted
   builds, so a tester application on a workstation writes none. An embedded target
   implements the trait against its own timer — ``embassy`` or a raw hardware counter — and
   the trait is the whole of what it must satisfy. This is the division ``UDSSVC_ARCH_0027``
   already records as the stack's pattern, citing ``uds_protocol``'s ``clap`` and ``utoipa``
   integrations as host conveniences that imply ``std`` and live behind features; this is the
   first time this crate invokes it for itself.

   That optional dependency needs ``UDSSVC_ARCH_0030`` amended. It currently states that this
   crate depends on no runtime crate, which becomes: none in the default build and none in a
   ``no_std`` build, with an optional and additive runtime behind a feature that a bare-metal
   target never enables. The ``no_std`` claim of ``UDSSVC_ARCH_0027`` is unaffected and is
   still evidenced the same way, on a ``*-none`` target.
