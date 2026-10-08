The seams
=========

What crosses each boundary of this crate, and who owns what on either side.

**Three seams, and this crate declares two of them.** ``UDSSVC_ARCH_0040`` made this crate
the driver, which settles who declares what by settling who calls whom:

.. list-table::
   :header-rows: 1
   :widths: 26 20 24 30

   * - Seam
     - Declared by
     - Implemented by
     - Element
   * - Service traits and callbacks
     - this crate
     - the consuming application
     - ``UDSSVC_ARCH_0012``, ``UDSSVC_ARCH_0033``, ``UDSSVC_ARCH_0038``
   * - ``UdsTransport``
     - this crate
     - a binding (``uds_on_ip``, ``uds_on_can``)
     - ``UDSSVC_ARCH_0029``, and ``UDSSVC_ARCH_0041`` for the time on it
   * - The response sink
     - ``automotive-wire-codec``
     - this crate's own buffer
     - ``UDSSVC_ARCH_0017``

The rule behind it is the one this page has given since the seams moved: **a seam is
declared by whoever the design makes responsible for it**, which for a driver means the
interfaces above and below are both its own. The sink is the exception and stays ``awc``'s,
because an I/O vocabulary shared by five crates belongs to none of them.

**Two earlier arrangements are recorded rather than deleted, because each is more plausible
than what replaced it.**

The first held that *"whoever is called declares the interface"* — the byte seam declared by
the binding because the binding calls in, the transport seam declared here because this
crate calls out. Tidy, and wrong about the first half: a seam carrying an addressing triple,
bytes, a sink and a responder is not transport-shaped, so every binding would have declared
the same trait separately.

The second replaced it with *"ownership follows the specifying document"*, and moved five
seams into ``uds_session`` on the ground that ISO 14229-2 specifies both interfaces of the
session layer. That was right about the document and wrong about the component. ISO 14229-2
names its service user as the ISO 14229-1 layer — this crate — so there was never a third
party between them for those interfaces to sit between. Once ``UDSSVC_ARCH_0040`` collapsed
the service user and the driver into one, four of the five seams stopped having two sides:
``RequestHandler``, ``PendingResponder`` and ``DiagnosticClient`` had nobody left to call
them, and ``Ctx`` stopped crossing a boundary at all. ``uds_session`` declares none of them
now, and says so in its own brief.

What survived that collapse is a library of ISO 14229-2 types and one state machine, which
this crate drives. ``uds_session`` declares no outward trait and calls nothing.

.. uml::
   :align: center
   :caption: The seams under the driver. Everything inside the ``uds_services`` box is
             internal; only the three interfaces on its boundary are seams.

   @startuml
   package "consuming application" {
     [typed service handlers] as APPH
     [tester code] as CLI
   }

   package "uds_services" {
     [driver loop] as DRV
     [dispatch pipeline] as PIPE
     [typed client] as TC
     [uds_session::Session] as SESS
   }

   package "binding (uds_on_ip, uds_on_can)" {
     [transport] as TR
   }

   interface "service traits" as ST
   interface "UdsTransport" as UT
   interface "awc::Sink" as SINK

   APPH -up-> ST
   CLI -down-> TC
   ST -down-> PIPE
   DRV -down-> PIPE
   DRV -right-> SESS : timestamps in,\nactions out
   TC -down-> DRV
   DRV -down-> UT
   UT -down-> TR
   PIPE -up-> SINK
   SINK -up-> APPH : handlers write here
   @enduml

Three things the diagram is meant to settle. The **sink is this crate's**, over a buffer
this crate sizes and bounded also by the peer's advertised maximum where it made one — it
was the driver's when the driver lived in the binding, and the driver moved. ``uds_session``
is **inside** the box: this crate owns the ``Session`` instance, supplies every input and
drains every action, which is why no arrow leaves the package for it. And the consuming
application touches exactly two things, the service traits and the typed client, which is
``UDSSVC_ARCH_0019``'s goal stated as a picture.

.. needflow::
   :filter: "seam" in tags
   :link_types: depends_on
   :align: center

Request context
---------------

.. arch:: There is no Ctx; the addressing triple is the pipeline's only extra input
   :id: UDSSVC_ARCH_0015
   :depends_on: UDSSVC_ARCH_0002
   :status: draft
   :origin: derived
   :tags: seam; ctx

   Every input the pipeline needs but cannot determine from the request bytes is exactly
   one thing: the ISO 14229-2 addressing triple, ``uds_session::Ai``. There is no request
   context struct. The driver takes the ``Ai`` from the ``S_Data.ind`` it drained and
   passes it into the pipeline beside the request bytes; everything else the mandatory
   decision nodes of ISO 14229-1:2020 Figures 5 and 6 and the suppression rules of clause
   8.7.5 read, this crate already holds.

   ``ServiceSet::dispatch`` takes the triple, beside the state ``UDSSVC_ARCH_0035`` keeps
   and the request bytes; the driver hands over the ``Ai`` it drained.

   Rationale: the pipeline is a function of the request and its addressing, so every clause
   8.7 rule is testable without a network, a clock or a session layer. That property was
   never what ``Ctx`` bought — it is what having *no* transport, clock or session layer in
   the signature buys, and a struct of one field does not add to it.

   **``Ctx`` is retired, and the whole of its reasoning is on the record because it was
   arrived at rather than assumed.** It was proposed as a seam: while something outside this
   crate called the pipeline, the inputs that pipeline could not derive had to arrive
   somehow, and a struct carrying one field per mandatory decision node of Figures 5 and 6
   is the disciplined way to write that — the field list is a checklist against the
   standard, and a check with no field is a check nobody implemented. It carried four
   things:

   .. list-table::
      :header-rows: 1
      :widths: 26 30 44

      * - Field
        - Read by
        - What became of it
      * - ``uds_session::Ai`` — the ISO 14229-2 addressing triple
        - Suppression (``UDSSVC_ARCH_0009`` rule 1) reads the target address type;
          ``UDSSVC_ARCH_0035`` keys per-channel state on the source address
        - Survives, passed on its own
      * - active session
        - Figure 5 → 0x7F, Figure 6 → 0x7E
        - Held here; not an input
      * - security level
        - Figure 5 and Figure 6 optional checks → 0x33
        - Held here; not an input
      * - authenticated
        - Figure 5 and Figure 6 → 0x34
        - Held here; not an input

   The three that went had been marked "under review" on a suspicion this element stated and
   did not close: that this crate implements ``DiagnosticSessionControl``,
   ``SecurityAccess`` and ``Authentication``, so under ``UDSSVC_ARCH_0035`` and
   ``UDSSVC_ARCH_0037`` it already holds the active session, the security level and the
   authentication state — and reading them back from a struct it just built from state it
   owns is a copy, not an input. All three resolved that way, none of them differently,
   which is the outcome the suspicion predicted. What remained was the addressing triple
   alone, and a struct wrapping one ``Ai`` is a rename rather than a type.
   ``UDSSVC_ARCH_0018`` had already removed the last reason for it to be nameable from
   outside; when the field list reduced to one, the struct went with it.

   Two things this does not weaken. The checklist argument was sound and is now owed
   elsewhere — nothing about deleting the struct implements Figures 5 and 6's checks, and
   the state they read being local makes it *easier* to forget a check, not harder, because
   no empty field remains to accuse anyone. And the pipeline's testability is unchanged for
   the reason given above. Open question 6, which asked what ``Ctx`` finally carried across
   the stack, is retired: nothing carries it, because there is nothing to carry.

   **Session and security are raw sub-function values, passed through uninterpreted.**
   Naming the sessions here would mean this crate deciding what
   ``DiagnosticSessionControl``'s sub-functions mean, which is ``uds_protocol``'s to define
   and the application's to choose.

   **Addressing is ``uds_session::Ai``, the ISO 14229-2 triple, used directly.** An earlier
   version of this element defined a local two-variant physical/functional type instead, on
   the ground that a binding's addressing triple is transport-shaped — ``uds_on_ip``'s
   carried a DoIP logical address — so embedding one would make a transport a mandatory
   dependency of the typed layer. That objection was to the *binding's* type, and it no
   longer applies: the triple now lives in ``uds_session``, shaped by ISO 14229-2:2021
   clauses 8.3–8.6 rather than by DoIP, and ``UDSSVC_ARCH_0002`` already depends on that crate.

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
   requires that "an authenticated state shall be linked to a certain diagnostic channel"
   and that "multiple clients can be handled on multiple channels with different
   authentication settings", so ``UDSSVC_ARCH_0035``'s per-channel state cannot be keyed
   without the source address. An earlier draft of this element excluded peer identity
   altogether and offered a security-access attempt counter as the example of state an
   application should key for itself — wrong twice over: clause 10.4 makes security access
   server-global, so it needs no key, and ``UDSSVC_ARCH_0034`` puts the counter in the stack
   rather than in the application.

   Taking ``Ai`` whole also settles ISO 14229-1 clause 7.4.1, which makes ``A_SA``, ``A_TA``
   and ``A_TA_Type`` mandatory parameters of every application layer service primitive. All
   three are present. The set argued about this for some time as though it were a question
   of what clause 8.7 reads; it was a question of which vocabulary the seam speaks, and it
   dissolved when the vocabulary moved to the crate that owns ISO 14229-2.

Outcome
-------

.. arch:: Dispatch is asynchronous and returns its outcome unsettled; the driver settles it
   :id: UDSSVC_ARCH_0016
   :depends_on: UDSSVC_ARCH_0009; UDSSVC_ARCH_0030
   :status: draft
   :origin: derived
   :tags: seam; outcome; async

   Rationale: a negative response is a normal, specified outcome of clause 8.7 expressed in
   the written bytes — a server answering 0x11 has succeeded at its job. Typing it as an
   error would put the most common non-trivial path through the crate into the ``Err``
   branch, and callers would learn to ignore errors. Dispatch, once its outcome is settled,
   instead reports whether a response was written and should be transmitted, or whether
   nothing is to be sent. A negative response is the first of those, not an error, and
   neither dispatch nor settlement has an error type at all; a sink refusal is a response
   too (0x14), not an error.

   Silence must be a distinguishable outcome rather than "wrote nothing", because the loop
   has to tell "clause 8.7 requires no response" apart from a response that was written.
   Only the first is a reason not to transmit.

   **How the cases are spelled.** ``dispatch`` returns the handler's outcome *unsettled*,
   as ``Unsettled``, and the driver applies clause 8.7's last stages to it: once the
   handler future has completed and before the response is submitted, it calls the
   pipeline's ``settle``, which applies "response fits" (``UDSSVC_ARCH_0017``) and
   ``UDSSVC_ARCH_0009``'s rules 1–3 and yields ``Responded`` — *responded* or
   *suppressed*, each carrying the session a ``DiagnosticSessionControl`` selected — and
   no ``Result``, so the driver's loop has two outcomes, not three. The ``Err`` arm this
   element once prescribed for the sink's failure is empty: the sink has one failure, a
   refused write, and a handler that writes too much sets the sink's refusal bit, which
   the pipeline's ``settle`` turns into ``responseTooLong`` (0x14) after ``rewind``,
   written bytes like any other negative response (``UDSSVC_ARCH_0017``).
   ``ResponseSink::MIN_BOUND`` guarantees that negative response fits: the sink's bound is
   never below it, and ``uds_server!`` rejects at compile time a response buffer, or a
   transport ``MAX_PDU``, shorter than it. The
   once-separate case, "something went wrong", has no remaining member. A negative
   response is therefore never in an ``Err`` branch a caller could learn to ignore, which
   is what this element's rationale asked for.

   **The driver settles, not dispatch, because rule 3's input is known only to the
   driver.** Whether a response-pending for this request was accepted for transmission is
   learned in the driver's loop while the handler runs (``UDSSVC_ARCH_0031``,
   ``UDSSVC_ARCH_0032``), and it is final only once the handler has finished. Settled
   inside the handler's future, that fact would have to be shared with the future while
   it is live; settled by the driver afterwards, it is a plain value passed once, and
   ``uds_server!``'s emitted ``dispatch`` routes and decides nothing.

   **``Responded`` is the whole report**, and no other outcome type stands behind it.
   Dispatch reports to the driver loop in this crate, not across a seam, for the reason
   ``UDSSVC_ARCH_0018`` gives. ``Unsettled`` and ``Responded`` are public only because
   the trait an application's assembly implements has to name the first and the driver's
   tests the second; an application never writes either.

   Genuine transport failures are the binding's concern, reach this crate as a ``DataConf``
   carrying an ``SResult``, and never reach a handler.

   **Dispatch and the handlers it calls are asynchronous**, which is what
   ``UDSSVC_ARCH_0030``'s assumption is spent on here. The gain is specific: a handler that
   takes longer than ``tP2_Server`` yields at its await points, so the driver keeps draining
   session actions while it runs and can submit a ``0x78`` (``UDSSVC_ARCH_0031``) in that
   window. With a synchronous handler the loop stops, the overrun is never observed, and the
   response-pending the standard allows is never sent.

   Without this, that is machinery every integrator would have to build. It is now machinery
   nobody builds, because the driver is here and the handler yields into it.

   The honest limit: an asynchronous seam does not make blocking work non-blocking. A
   handler that busy-waits on a flash erase stalls the executor exactly as a synchronous
   one would, and the gain is real only for handlers written to yield. What changes is that
   yielding is now *possible* at the seam, where before it was not expressible at all.

Response sink
-------------

.. arch:: A response is written into a sink, never returned owned
   :id: UDSSVC_ARCH_0017
   :depends_on: UDSSVC_ARCH_0001; UDSSVC_ARCH_0029
   :status: draft
   :origin: derived
   :tags: seam; no_std

   Rationale: allocation-freedom cannot be retrofitted, because the signatures that make an
   API alloc-free are the ones callers depend on: adding it later is a breaking change to
   every handler in every application. A handler therefore writes its response into an
   ``automotive_wire_codec::Sink`` it is handed, rather than returning an owned response. No
   public type carries a ``Vec`` or a ``String``, and the crate builds under ``no_std``
   without ``alloc``.

   **The sink is this crate's**, which is a change of owner rather than of design. It was
   the binding driver's while the driver lived there; ``UDSSVC_ARCH_0040`` moved the driver
   here, so the buffer a response is assembled in is this crate's to hold and to bound.

   **Where the storage comes from is now settled, and this element left it open.** It used
   to say that a caller-supplied buffer and one sized at assembly alongside
   ``UDSSVC_ARCH_0035``'s protocol state were both admissible and that the choice was a
   deployment question. It is not a deployment question, because only one of the two is
   reachable: ``UDSSVC_ARCH_0013``'s macro folds each service's declared maxima into array
   lengths at a site where the types are concrete, and the driver holds the resulting arrays
   inline. **The application picks no number and passes no buffer.** That is a stronger
   property than the open version allowed for — a caller-supplied buffer can be too small
   for the very response the server's own declarations say it may produce, and there is no
   size an application could pick that the crate could check against, whereas a folded one
   cannot be wrong by construction.

   There are **three** such buffers, not one, and the reason is ``UDSSVC_ARCH_0031``'s
   response-pending window rather than anything about responses. A handler holds a request
   borrowing the in-flight buffer, so that buffer cannot be lent back to the transport while
   the handler runs; since ``UDSSVC_ARCH_0041`` retired the clock seam, waiting on the
   transport is the only way this crate can wait, so without a second buffer the
   ``tP2_Server`` deadline is never observed and no 0x78 is ever sent. The third is the
   response. Clause 8.7.6's two exceptions are a second use for the concurrent buffer, not
   the reason it exists.

   **The sink is ``awc``'s rather than ``embedded-io``'s, and the difference produces an
   NRC.** A response is written through a sink bounded at the peer's maximum payload where
   one is advertised; an over-long response fails at the write with the counts intact, and
   this crate is to turn that counted failure into ``responseTooLong`` (0x14). That settles
   a question the set had been carrying: the maximum is a transport property this crate does
   not know, but the negative response code is a clause 8.7 outcome that is nobody else's.
   ``embedded-io`` leaves this crate's dependency list entirely.

   **The bound is never below three bytes.** ISO 14229-1 fixes a negative response at
   three bytes; a peer that cannot receive three bytes cannot take part in UDS at all, so
   a bound below that is not one the protocol admits and is raised to it. The pipeline's
   own ``7F <sid> <nrc>`` is therefore never refused. A handler's refused write is
   recorded by the sink and read by the pipeline after the handler returns; the handler
   itself ignores the write's result, there being nothing it could add.

   **Built for the stages that exist.** ``ResponseSink`` holds the bound and rejects a
   write past it, and the pipeline's settle step maps the rejection to 0x14 after
   rewinding the sink. The services without a pipeline stage settle 0x11 and never reach
   that mapping.

   **This element's account of that failure type has now been wrong three times, and the
   count is the point.** The versions, in order:

   1. "``Sink`` carries ``remaining()``." It does not. In ``automotive-wire-codec`` 0.4.0
      the trait carries ``write_all`` alone; ``remaining()`` is an inherent method on
      ``SliceSink`` and ``Limited``, so a handler generic over ``S: Sink`` cannot call it.
   2. "``InsufficientBuffer``'s ``needed`` and ``available`` counts." There is no
      ``needed``. The field is ``needed_at_least``.
   3. The correction to (2) was still wrong about what the number *means*. ``awc`` 0.4.0
      documents ``needed_at_least`` as a **lower bound** on the encode's total size — the
      encode stopped at the write that failed, so whatever remained was never measured. An
      exact total requires ``Encode::encoded_size``.

   The mechanism has survived all three, for the same reason each time: 0x14 carries no
   length, so this crate needs the failure to be *counted* only in the sense of being
   distinguishable from every other write failure, and never needs the count itself. That is
   also why each error was cheap enough to survive review — nothing downstream reads the
   field, so naming it wrongly broke nothing and compiled nothing. Three misses at a
   member no code path depends on is evidence about this document rather than about the
   design: a claim nothing tests is a claim that stays wrong. A reader should treat the
   field names here as the least reliable sentences on the page and check them against
   ``automotive_wire_format/src/error.rs``.

   **The bound itself is less solid than this element assumed.** On DoIP the number is *Max.
   data size*, which ISO 13400-2:2019 Table 11 lists as an **optional** item of the entity
   status response — so a conformant entity may advertise none — and defines as "the maximum
   size of one logical **request** that this DoIP entity can process", which is the inbound
   direction rather than the outbound one a response occupies.

   Two consequences to design around rather than assume away. There may be **no bound to
   apply**, in which case 0x14 is unreachable and a response is limited only by the sink the
   caller supplied; that is a correct outcome, not a degraded one. And where a bound does
   exist, **whose it is** — this server's or the peer's — is the transport's question to
   answer, not this crate's. What must not happen is a fabricated default, which would make
   a conformant server truncate valid responses in order to produce an NRC nothing asked
   for.

   The sink is a **concrete type, ``ResponseSink``** — neither ``dyn`` nor generic — and
   that too is a change this element should record rather than quietly adopt. It said the
   sink was a **generic parameter, not ``dyn``**, and weighed the lost object safety: no
   ``Box<dyn ServiceSet>``, the driver generic over the handler type, defensible for a crate
   that cannot box anyway. Both halves of that trade are gone, because the premise under
   them was that the sink *might be the caller's*. Once the storage question above closed,
   it cannot be: this crate owns the response buffer, so there is exactly one sink type and
   a ``<S: Sink>`` parameter abstracts over a set with one member. Dropping it removes a
   type parameter from every service trait, every generated impl and the driver, and costs
   nothing at run time, since dispatch was fully monomorphised either way. The object-safety
   observation survives unchanged and is simply owed by a different type now: ``ServiceSet``
   is not object-safe, and a dynamic service registry would still be a redesign.

   One consequence to design against rather than discover: a sink can fail mid-response,
   after some bytes are already written. ``awc``'s decision to expose ``write_all`` rather
   than ``write`` removes the *partial write* case, but not the *failed part-way through*
   case. What remains of the question is narrower than this element used to state it. It
   asked whether "the binding must discard a partial response" — which needed a binding's
   agreement. It does not any more: this crate owns both the buffer and the transport, so a
   partial response is discarded by the only mechanism that matters, which is **not
   submitting it**. Nothing is handed to ``uds_session`` or to the transport until the
   handler has returned and the outcome says bytes are to be sent, so a failed response
   never reaches the wire and the bytes in the buffer are simply overwritten by the next
   one. What is still genuinely open is whether a *transport* must be told that a response
   it was never offered was abandoned — which matters only where a transport has already
   been made aware of an impending response, and on ``UDSSVC_ARCH_0029``'s seam it has not.
   See :doc:`open-questions`.

   ``no_std`` freedom is verified on a bare-metal target, not by
   ``--no-default-features`` on a hosted one. Only a ``*-none`` target proves ``std`` has
   not crept back in through a dependency.

The transport seam
------------------

.. arch:: This crate declares the transport seam and calls it
   :id: UDSSVC_ARCH_0029
   :depends_on: UDSSVC_ARCH_0002; UDSSVC_ARCH_0030; UDSSVC_ARCH_0040
   :status: draft
   :origin: derived
   :tags: seam; transport; client; driver

   One trait sits below this crate, declared here and implemented by a binding:

   .. code-block:: rust

      pub trait UdsTransport {
          /// What this transport's failures are. Never interpreted by this crate.
          type Error;

          /// The largest A_PDU this transport can carry, where its protocol caps it.
          /// Participates in UDSSVC_ARCH_0013's const fold.
          const MAX_PDU: usize = usize::MAX;

          /// T_Data.req — hand a T_PDU to the transport, and say what follows it.
          async fn t_data_req(&mut self, ai: Ai, data: &[u8], after: AfterSend)
              -> Result<(), Self::Error>;

          /// Fill `buffer` with the next inbound message, or return
          /// `TransportEvent::Deadline` when `deadline` passes first.
          async fn next_event(&mut self, buffer: &mut [u8], deadline: Option<Timestamp>)
              -> Result<TransportEvent, Self::Error>;

          /// The largest response the peer will accept, where it advertised one.
          fn outbound_max(&self) -> Option<usize>;

          /// The tP_Client reload pair this transport dictates.
          fn channel_timing(&self) -> Reloads;

          /// Monotonic milliseconds, 32-bit and wrapping (UDSSVC_ARCH_0041).
          fn now(&self) -> Timestamp;
      }

   ``Ai``, ``SResult``, ``Reloads`` and ``Timestamp`` are ``uds_session``'s. **Nothing in
   the trait is DoIP-shaped, which is the test of whether it is the right seam** — a CAN
   binding implements the same five methods, and ``uds_on_can`` becomes additive with no
   change here.

   **Five changes from the version above, and the first is the one that mattered.**

   *``next_event`` fills a caller's buffer instead of lending one.* It used to return
   ``TransportEvent<'_>``, borrowing the transport's own receive buffer, which is the
   obvious shape and does not work. The borrow lives exactly as long as the request does,
   and the request lives until a response has been composed from it — so for the whole of
   that window the transport is mutably borrowed and ``t_data_req`` cannot be called. **The
   driver could never send the response the request provoked.** It is not a borrow-checker
   inconvenience with a workaround; it is the seam saying the design was wrong, because a
   request-response protocol whose request pins the channel it must be answered on cannot
   run. The buffer is the driver's (``UDSSVC_ARCH_0017``'s three), the event carries a
   length into it, and the borrow ends when the call returns. ``TransportEvent`` has no
   lifetime parameter at all now, which is what keeps the property from being reintroduced
   by accident.

   *``timeout_ms`` became a ``Timestamp`` deadline.* ``UDSSVC_ARCH_0041`` carries the
   argument and retires the one that put a duration here.

   *``max_payload`` became ``outbound_max``, and no ``inbound_max``.* One number could not
   answer both questions because the standard's number is **request-shaped**: ISO
   13400-2:2019 Table 11 defines *Max. data size* as "the maximum size of one logical
   request that this DoIP entity can process". A server asking what it may *send* is
   therefore asking about the client's advertisement, not its own, and the two are
   different values belonging to different entities. Collapsing them bounds a response by
   the server's own receive capacity, which is a limit nothing in the standard imposes.
   ``outbound_max`` is what the peer advertised, read *up*; a tester never advertises one
   to an entity, so over ``DoIP`` it is ``None``. ``MAX_PDU`` is a different thing: a
   protocol-fixed cap known at compile time, which is why it is an associated const and
   joins the fold.

   **There is deliberately no ``inbound_max``.** It was meant to hand this crate's in-flight
   buffer length down for the transport to refuse longer requests with, and built that way
   (``luminartech/uds_stack#41``) it took the longest *well-formed* request as its limit, so
   an over-long request for an unsupported service got a transport refusal where ISO
   14229-1 has the server answer ``serviceNotSupported`` (0x11) before
   ``incorrectMessageLengthOrInvalidFormat`` (0x13). A request longer than the in-flight
   buffer instead reaches the server as ``TransportEvent::DataTooLong``, and the pipeline
   answers it in that order; a transport refuses only what it cannot hold itself.
   ``uds_on_ip``'s ``ARCHITECTURE.md`` §4.1 records the ``DoIP`` side.

   *``t_data_req`` gained ``after: AfterSend``.* ISO 14229-5:2022 REQ 7.9 has a server
   close its TCP connection after a ``DiagnosticSessionControl`` positive response *if* the
   session change disconnects it, and what disconnects it is the server leaving the software
   it runs
   — an application jumping to its bootloader, a bootloader restarting the application
   (ISO 14229-1:2020 10.2.2.2 Table 25). That is a property of the server, not of the
   response's octets, so a transport cannot derive it and the server says it:
   ``ServerLeaves`` on the one final positive response whose session
   ``DiagnosticSessionControl::leaves_running_software``, ``Continue`` on everything else,
   0x78 and negative responses included. An enum rather than a ``bool``, and transport-
   neutral: a transport without connections ignores it, and this crate still names no
   transport (``UDSSVC_ARCH_0002``). ``ECUReset`` is never flagged — REQ 7.11's close follows
   *every* ``ECUReset`` positive response, so a ``DoIP`` transport keys it on ``0x51``
   itself. A suppressed session change sends no response, so nothing carries the flag and
   the connection drops unannounced; that is recorded at the method, with no mechanism.

   *``Closed`` gained ``peer: Address``.* A ``DoIP`` server carries several testers'
   connections behind one transport, and the driver ended the exchange it was serving on any
   close, so one tester leaving abandoned another's request. Naming the peer lets it end
   only the exchange the close belongs to.

   *``now_ms() -> u32`` became ``now() -> Timestamp``.* Same obligation, a type that carries
   ``UDSS_LLR_0019``'s modular arithmetic with it.

   *Three variants were added, and two of them are conformance requirements rather than
   conveniences.*

   ``DataTooLong`` reports a message longer than the buffer offered, with what fit and —
   where the transport can know it without reading the whole message, as DoIP can from its
   generic header — how long it really was. It is a **variant rather than a flag** on
   ``DataInd`` because of how each fails: destructuring ``DataInd { ai, len, .. }`` is
   idiomatic and would silently discard a truncation flag, leaving a fragment decoded as a
   complete message, whereas an unhandled variant drops a message the client is already
   required to repeat. One fails dangerous, the other safe. It is also not an error
   condition: a driver serving a request offers only the small concurrent buffer, so
   truncation is the *normal* outcome there. Clause 8.7.6 has the server occupied and names
   no answer; ``busyRepeatRequest`` (0x21), Figure 5's busy check, is the conforming one
   this stack gives, and needs only the service identifier and the addressing.

   ``Periodic`` **cannot be a ``DataInd``**. ISO 14229-5:2022 REQ 7.20 requires a periodic
   response not to reset ``tS3_Server``, and ``DataInd`` is precisely what is fed to the
   session layer and resets it. Delivering one as a ``DataInd`` is a conformance failure,
   not a shortcut, so the variant is forced by the standard rather than chosen for tidiness.
   Its ``0x8004`` payload type is DoIP-specific but its *shape* is not — a CAN binding
   implementing periodic responses reports the same three things.

   ``Closed`` **cannot be an ``Err``**. ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a
   server-initiated close part of the normal ``DiagnosticSessionControl`` and ``ECUReset``
   flows — after every positive ``ECUReset`` response, and after the positive
   ``DiagnosticSessionControl`` response to a session change that leaves the running
   software, which the driver marks ``AfterSend::ServerLeaves`` — so an expected close is a
   step in a prescribed sequence and not a failure. Typing
   it as an error would put a conformant flow into the branch ``UDSSVC_ARCH_0016`` argues
   callers learn to ignore — the same mistake, one seam lower. It carries whose connection
   closed and whether the close was expected. The peer scopes it: a server serving one
   client's request while another client's connection closes ends nothing, and a transport
   that cannot name a connection's peer reports no close for it. Whether it was expected is
   informational: re-establishing the connection is never this crate's. ISO 14229-5:2022
   REQ 7.8 and REQ 7.10 have the client open a new connection and activate routing again,
   so a server is reconnected *to* and never reconnects, and a client's own reconnection is its transport's business below this seam. A transport
   with no connections never emits it, exactly as one that never truncates never emits
   ``DataTooLong``.

   Rationale: this crate calls out to a transport, so this crate declares what it calls. The
   trait is a transport's whole obligation to the stack: carry bytes in both directions, say
   how large a payload it will take, say what timing it dictates, and tell the time. It
   carries no notion of a service, a data identifier or a negative response code, which is
   what keeps a binding from needing to understand UDS.

   **It serves both roles, and that is a change from what it replaced.** The client's
   exchanges and the server's inbound requests are the same bytes on the same transport;
   only what this crate does with them differs. A separate client trait would have obliged a
   binding to implement two interfaces to the same socket.

   The asynchrony is ``UDSSVC_ARCH_0030``'s: the trait is ``async`` and the runtime is the
   caller's, so this crate still depends on no executor.

   **Three earlier versions of this element are on the record, and the middle one is the
   instructive failure.**

   It first declared ``UdsTransport`` here, argued as the exact inverse of a byte seam the
   binding declared — that symmetry was real and rested on a premise that did not survive.
   It then moved to ``uds_session`` as ``DiagnosticClient``, on the ground that an ISO
   14229-2 service primitive belongs to ISO 14229-2's crate. That reasoning was sound about
   the *document* and wrong about the *component*: ISO 14229-2 names its service user as the
   ISO 14229-1 layer, so with ``UDSSVC_ARCH_0040`` making this crate both the service user
   and the driver, ``DiagnosticClient`` had no caller on the other side. It is back, and it
   is one trait rather than two.

   The functional case that justified splitting the old client trait into two methods
   survives as a property of this crate's client surface rather than of the transport.
   ``UDSSVC_ARCH_0022``'s lending sequence is assembled here, from however many ``DataInd``
   events arrive before the window closes; a transport reports each one and counts nothing.

There is no handler seam
------------------------

.. arch:: Nothing calls into this crate; the inbound path is the driver's own
   :id: UDSSVC_ARCH_0018
   :depends_on: UDSSVC_ARCH_0029; UDSSVC_ARCH_0040
   :status: draft
   :origin: derived
   :tags: seam; transport; driver

   No trait exists by which something outside this crate hands it a request. The driver of
   ``UDSSVC_ARCH_0040`` reads a ``TransportEvent::DataInd`` from ``UDSSVC_ARCH_0029``'s
   transport, submits it to the ``uds_session::Session`` it owns, takes the resulting
   ``S_Data.ind``, and calls its own dispatch pipeline. Every step of that is internal.

   Rationale: recorded as an element rather than as an omission, because a handler seam is
   the single most likely thing to be re-proposed here. Two full design cycles produced one
   — first declared by each binding, then declared by ``uds_session`` as ``RequestHandler``
   — and both followed from the same unexamined premise: that something below this crate
   turns the crank and calls upward. Nothing does. ISO 14229-2 defines an interface between
   the session layer and its service user and names that user as the ISO 14229-1 layer, so a
   crank-turning third party is a component no standard describes.

   Two things that used to cross this seam are now internal, and are named here because
   their elements still describe them as though they crossed something: ``Ctx``
   (``UDSSVC_ARCH_0015``) is the dispatch pipeline's input, constructed here from the
   session layer's indication; ``Responded`` (``UDSSVC_ARCH_0016``) is what dispatch reports
   to the loop around it. Neither is a seam: ``Responded`` is public only because the
   assembly's trait names it.

   **What this does not change** is the pipeline's shape. ``UDSSVC_ARCH_0004`` is still a
   pure function of a request and its context, still testable without a transport, a clock
   or a session layer. Removing the seam removed a trait, not a boundary — the boundary is
   still there, it is just not a public one.

Response-pending
----------------

.. arch:: A response-pending is an ordinary transmission, not a seam
   :id: UDSSVC_ARCH_0031
   :depends_on: UDSSVC_ARCH_0016; UDSSVC_ARCH_0029; UDSSVC_ARCH_0032
   :status: draft
   :origin: derived
   :tags: seam; async; response-pending

   When ``UDSSVC_ARCH_0032`` decides a ``requestCorrectlyReceivedResponsePending`` (0x78) is
   admissible and composes its three octets, they go out the way every other response does:
   submitted to the ``uds_session::Session`` this crate owns as an ordinary ``S_Data.req``,
   and transmitted through ``UDSSVC_ARCH_0029``'s transport when the resulting action is
   drained. There is no separate interface and nothing new in anyone's surface.

   The moment is known the same way. ``uds_session`` reports the ``tP2_Server`` overrun
   (``UDSS_LLR_0117``) as one of the outputs the driver already drains, so dispatch learns
   that a response-pending is due from the loop it is already running, not from a trait it
   is handed. It reports the overrun ``response_pending_lead`` before the window closes
   (``UDSS_LLR_0186``), so the 0x78 is issued within it.

   Rationale: the decision and the bytes are this crate's (``UDSSVC_ARCH_0032``) and so is
   the transport (``UDSSVC_ARCH_0029``), so nothing crosses a crate boundary and a seam
   would have two sides in the same crate. What remains true, and is what this element is
   really about, is the **division of labour with the session layer** — unchanged by the
   seam's removal:

   .. list-table::
      :header-rows: 1
      :widths: 40 60

      * - This crate answers
        - ``uds_session`` answers
      * - *Whether* a 0x78 is admissible, and *what* bytes it is
        - *When* one is due, whether a submission is accepted, and how often one may repeat

   The session layer's constraints are where they always were and are the reason a
   submission can be refused: it rejects one while a predecessor is unconfirmed
   (``UDSS_LLR_0118``), spaces consecutive ones (``UDSS_LLR_0119``), refuses a duplicated or
   exhausted transmission association (``UDSS_LLR_0061``, ``UDSS_LLR_0062``), and anchors on
   a confirmed transmission (``UDSS_LLR_0110``). A refusal is reported synchronously, at the
   moment of submission, which is what ``UDSSVC_ARCH_0009``'s gate needs: a refused
   submission is not a send, so the suppression override must not fire on it.

   **An earlier version of this element declared a ``PendingResponder`` trait** with a
   cancel-safe ``due()`` and an ``offer()`` returning ``bool``, declared by ``uds_session``
   and implemented by a binding's driver. It described a seam between this crate and a
   component that no longer exists. Two of its arguments survive intact and are worth
   keeping, because both were arrived at rather than assumed:

   * **Submission and confirmation are two moments, not one.** ``UDSS_LLR_0114`` stops
     ``tP2_Server`` at the ``T_Data.req`` and ``UDSS_LLR_0110`` anchors spacing at the
     ``T_Data.conf``. Dispatch must not wait for the second, or a slow handler stalls for
     the duration of a transmission — the opposite of what ``UDSSVC_ARCH_0016``'s
     asynchronous seam buys.
   * **Refusal and failure are different answers.** A synchronous refusal means nothing was
     sent. An accepted submission whose transmission later fails is reported asynchronously
     as a ``T_Data.conf`` and is absorbed by the loop; ``UDSS_LLR_0110`` has such a
     transmission never reach the data link, so the client saw no response-pending and an
     unsuppressed final response is harmless.

   The asymmetry that follows is ``UDSSVC_ARCH_0009``'s to state: treating an accepted
   submission as sent can cost one message a waiting client accepts, while treating a sent
   one as unsent produces silence where the standard requires a final response.

Clock
-----

.. arch:: Time arrives on the transport seam; there is no clock seam
   :id: UDSSVC_ARCH_0041
   :depends_on: UDSSVC_ARCH_0029; UDSSVC_ARCH_0030; UDSSVC_ARCH_0040
   :status: draft
   :origin: derived
   :tags: seam; async; driver; time

   Driving ``uds_session`` requires time: it is sans-io and evaluates a timer's expiry only
   when a timestamp is supplied (``UDSS_LLR_0017``). So ``UDSSVC_ARCH_0040``'s loop needs to
   know what time it is, and needs to wake when something is due. **Both come from the
   transport, which is asked for them as part of the seam it already implements.** This
   crate declares no clock trait and ships no clock implementation.

   Two obligations, and the second is not new:

   * ``now() -> Timestamp`` — monotonic milliseconds, 32-bit and wrapping.
   * The existing "give me the next event, or wake me when this deadline passes" call,
     whose deadline this crate passes through.

   Rationale: a transport that can report an inbound event *or* a timer expiry, whichever
   comes first, already measures time — that capability is what the seam asks for, not
   something added to it. A separate ``Clock`` trait would therefore not supply a capability
   the transport lacks; it would duplicate one the transport must already have, and admit
   two implementors holding two time bases that have to agree silently. That is the
   arrangement ``UDSSVC_ARCH_0002`` refuses for session state, for the same reason: two
   things tracking one fact is how they come to disagree.

   It is also where the platform integration already is. Whoever writes a transport is
   already reaching for sockets and an executor on that target; asking the same implementor
   for the clock adds no new platform surface, while a second trait would be a second thing
   every target has to satisfy.

   **The seam carries a deadline, and the argument that it should carry a duration is
   retired.** That argument was this element's longest and is kept on the record because it
   was correct about everything except the type. It ran: a deadline in wrapping ``u32``
   space is ambiguous on its own, since ``5`` is either a moment just past or one roughly 49
   days away and only a reference point separates them; this crate holds both values — the
   deadline ``uds_session`` reports under ``UDSS_LLR_0080`` and the ``now_ms`` it just read
   — so it should compute the interval here, once, by the modular subtraction
   ``UDSS_LLR_0019`` already specifies, and pass a number that cannot be misread. Handing
   the transport a raw deadline would otherwise oblige every implementor to rederive that
   arithmetic identically, which is the same objection whichever side of the seam holds the
   clock.

   What retires it is that the arithmetic no longer has to be rederived by anyone.
   ``uds_session::Timestamp`` is not a bare ``u32``: ``Timestamp::interval_since`` performs
   exactly the modular subtraction the paragraph above worried about, and a ``Timestamp``
   carries its own interpretation across the seam. So the premise — that a deadline is a
   number an implementor must know how to read — is false for this type, and with it the
   conclusion. Passing ``uds_session::Server::next_deadline``'s value through untouched is
   then strictly better than computing an interval from it, on two counts the duration form
   could not offer. It is **lossless**: an interval is computed against a ``now`` sampled at
   one instant and consumed at a later one, so every duration crossing the seam is stale by
   however long the crossing took, while a deadline means the same thing whenever it is
   read. And it removes a step rather than moving one — *neither* side computes an interval
   now, where the duration form had this crate doing it on every iteration.

   The ambiguity the old argument identified was real. It was a property of ``u32``, and the
   fix was to stop using one.

   **An earlier version of this element declared a ``Clock`` trait here**, with ``now_ms``
   and an ``async sleep(ms)``, and shipped a tokio-backed implementation behind a feature.
   Two things retire it. ``sleep`` was redundant: the driver never sleeps independently of
   waiting for input, so the transport's own "next event or timeout" call already *is* the
   wait, and the clock contributes exactly one method rather than two. And the shipped
   implementation was the only reason this crate would have carried an optional runtime
   dependency — with the clock on the transport, ``uds_on_ip`` supplies it and this crate
   ships nothing.

   That last point is worth recording because it reverses a change the briefs had queued.
   ``UDSSVC_ARCH_0030`` states that this crate depends on no runtime crate; the retired
   element required it amended to "none in the default or ``no_std`` build, with an optional
   runtime behind a feature". **No amendment is owed.** ``UDSSVC_ARCH_0030`` stands as
   written, in every build, which is a stronger claim than the amendment would have left.

   The reason a clock is a seam at all is unchanged, and is ``UDSS_LLR_0017``'s: reading one
   directly would make every timing rule in the stack untestable except in real time. Taking
   it across a seam keeps a test able to advance time by returning larger numbers — and
   taking it across *this* seam is better for that than a separate trait would have been,
   because one fake supplies the events and the time together and cannot make them disagree.

   The cost, stated rather than discovered: "can tell the time" is now coupled to "is a
   transport", so a deployment driving the stack over a channel it would not otherwise model
   as a transport still implements ``now``. Given the seam already demands a deadline, that
   is not a new burden.
