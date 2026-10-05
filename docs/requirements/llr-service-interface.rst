Service interface
=================

Requirements defining the session layer's service interface: the primitives exchanged
with the application and with the transport, the parameters those primitives carry, and
the mapping between them.

ISO 14229-2 describes the session layer as a service provider that the application
calls. This crate is sans-io, so nothing is called: the caller supplies inputs and
retrieves outputs. The requirements below keep the standard's names for the primitives
and state separately how each one is realised, so that the trace to the standard stays
direct.

Throughout this document, the **caller** is the code that owns the session layer
instance and drives it, typically the integration layer that also owns the transport.
The **application** is the diagnostic application on whose behalf the session layer
transmits and receives messages.

Requirements transcribed from ISO 14229-2 speak of the application passing a primitive to
the session layer, or the session layer passing one to the application, because that is how
the standard describes a service interface. Read those statements as fixing where a
primitive comes from and where it goes, not as describing a call: the caller supplies every
input on the application's behalf and retrieves every output for it, as ``UDSS_LLR_0009``
and ``UDSS_LLR_0011`` require. ISO 14229-2's *service user* is the application in this
document's terms.

Sans-io binding
---------------

ISO 14229-2 does not contemplate a sans-io implementation, so these requirements are
derived. They fix the boundary that every other requirement in this set is written
against. The primitives they refer to are defined in `Service primitives`_ below.

Note the two senses of "input" and "output" in this document. ``UDSS_LLR_0001`` uses I/O
in the operating-system sense, of reading and writing a device. ``UDSS_LLR_0009`` and
``UDSS_LLR_0011`` use input and output in the state-machine sense, of values passed to
and retrieved from the session layer. The first is forbidden; the second is the whole
interface.

One assumption of use falls on the order in which the caller supplies inputs, and is
recorded in the qualification repository: a ``T_Data.conf`` is supplied before any
``T_DataSOM.ind`` or ``T_Data.ind`` the transport received after the confirmed transmission
completed. A transport reports the two in that order, and a caller draining one queue
before the other is what the assumption forbids. A transmission completes here when the
transport can report it, and the assumption does not order a confirmation before an
indication of a message the transport received before then: ISO 14229-2:2021 10.3 lets the
client send its next request on complete reception of the response, which
ISO 14229-2:2021 9.2 REQ 5.19 does not exclude, so that request can be indicated before the
previous response is confirmed. ``UDSS_LLR_0088``, ``UDSS_LLR_0093`` and ``UDSS_LLR_0109``
state what the server does with that overlap.

.. llr:: The session layer performs no I/O
   :id: UDSS_LLR_0001
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not open, read, or write a transport, socket, file, device, or
   operating system service.

   Rationale: the crate is sans-io. Every interaction with the vehicle network belongs
   to the caller, so one implementation serves CAN, DoIP, K-line and simulation alike.

   This requirement is verified by inspection of the crate's own sources rather than by a
   runtime test; no black-box test can show that no I/O is performed.

.. llr:: The crate compiles under no_std
   :id: UDSS_LLR_0002
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The crate shall compile under ``no_std``.

   Rationale: a crate can perform no I/O of its own, as ``UDSS_LLR_0001`` requires, and
   still be unusable on the targets this set is written for, because it names a type that
   only ``std`` provides. The two are independent, and this one is verified by a build for
   a target without ``std`` rather than by inspection: the compiler answers it exactly, and
   a regression shows up as a failed build rather than as a reviewer's omission.

.. llr:: The crate declares no dependency that performs I/O
   :id: UDSS_LLR_0003
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The crate shall declare no dependency that performs I/O.

   Rationale: ``UDSS_LLR_0001`` binds what the crate's own sources do, and a dependency's
   sources are not the crate's; without this requirement the prohibition could be kept to
   the letter by a dependency that opened a socket. It is verified by review of the
   dependency graph rather than by a build, no build failing over a dependency that works.
   ``Cargo.toml`` declares an empty ``[dependencies]``, so the requirement is satisfied
   today by there being nothing to review.

.. llr:: The session layer allocates no memory
   :id: UDSS_LLR_0004
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall allocate no memory.

   Rationale: the quantities this set keeps state for are properties of the deployment
   rather than of the protocol — the number of a client's channels, which ``UDSS_LLR_0120``
   holds a timer in caller storage for, the number of peers an instance addresses, which
   ``UDSS_LLR_0059``'s associations are held per, and the number of responders answering
   behind one functional address, which ``UDSS_LLR_0139``'s table is sized for — so the
   crate cannot know them and the caller sizes them. Each of those requirements places its
   state in caller-supplied storage and gives this property as the reason; until it was
   stated, the reason was given in rationale alone, and an implementation could have
   satisfied ``UDSS_LLR_0126`` with a heap-backed map without contradicting any requirement.
   This requirement is verified alongside ``UDSS_LLR_0002`` and ``UDSS_LLR_0003``, by the
   build configuration and the dependency graph, rather than by a runtime test. Where a fact
   may be kept instead is ``UDSS_LLR_0008``'s, which this requirement leaves with only two
   places to name. Supplying that storage by value instead of by borrow changes nothing
   here: the caller still sizes each quantity, now at compile time, so "the crate cannot
   know them and the caller sizes them" holds unchanged.

.. llr:: The crate contains no unsafe code
   :id: UDSS_LLR_0005
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The crate shall contain no ``unsafe`` code.

   Rationale: every claim this set makes about the session layer's behaviour is a claim
   about what safe Rust guarantees of the compiled crate, and one ``unsafe`` block puts all
   of them beyond the compiler's reach at once. ``Cargo.toml`` answers this requirement with
   ``unsafe_code = "forbid"``, which a local attribute cannot switch off; the requirement is
   what that setting is accountable to, so that removing the setting is a defect against a
   stated property rather than an unreviewed change to a build file.

.. llr:: Every input is processed or rejected, and none aborts
   :id: UDSS_LLR_0006
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   For every input the caller supplies, the session layer shall either process it or reject
   it under ``UDSS_LLR_0015``. No input shall cause the session layer to abort or to fail to
   return.

   Rationale: this is the global form of the argument ``UDSS_LLR_0019`` makes locally for
   one operation. Without it the set is silent on what happens to an input no requirement's
   conditions select, and silence there is indistinguishable from a panic — which in a
   diagnostic server is an unhandled failure of a safety-related component, and in a caller
   that cannot unwind is a halt. Stating it as totality also makes it testable: an input
   either yields outputs or yields a rejection report, and both are observable.
   ``Cargo.toml`` denies ``unwrap_used``, ``panic``, ``indexing_slicing`` and
   ``arithmetic_side_effects``, which remove the common ways of breaking this property; they
   are the means, and the requirement is the claim they serve.

.. llr:: The session layer is deterministic
   :id: UDSS_LLR_0007
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The same sequence of inputs supplied from creation shall yield the same state and the
   same outputs, in the same order, except where a requirement in this set leaves an order
   unspecified. ``UDSS_LLR_0081`` leaves one such order open: where one timestamp causes
   several expiries, the order of the indications they produce is not specified.

   Rationale: this is what makes every requirement in the set testable without a network, a
   clock or a scheduler — a property available because ``UDSS_LLR_0001`` leaves every
   interaction with the vehicle network to the caller, and one ``UDSS_LLR_0009`` secures by
   admitting no source of state but an input. The exception is needed because a flat claim
   would contradict ``UDSS_LLR_0081``, which argues that several expiries on one timestamp
   need no order for the state they leave; an implementation is free there, and a test must
   be written to accept either order rather than to fix the one it happened to observe.
   ``UDSS_LLR_0073`` states the slice of this claim that concerns the message payload, as an
   equivalence over pairs of inputs differing only in that payload; that is a statement
   about which inputs may differ, this one about repeating the same inputs, and neither
   implies the other.

.. llr:: All state lives in the instance or in caller-supplied storage
   :id: UDSS_LLR_0008
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   All state the session layer keeps shall live in the instance or in storage the caller
   supplies. The session layer shall retain nothing else between inputs.

   Rationale: ``UDSS_LLR_0004`` forbids the session layer to allocate; this fixes where what
   it keeps lives instead, and adds that it retains nothing else — the part a static or a
   process-wide table would break without allocating. Together they close the inventory, so
   that a reader looking for where a fact is kept has two places to look and no third: the
   instance, as ``UDSS_LLR_0104`` and ``UDSS_LLR_0082`` hold the server's state, or
   caller-supplied storage, as ``UDSS_LLR_0126``, ``UDSS_LLR_0059``, ``UDSS_LLR_0150`` and
   ``UDSS_LLR_0151`` hold the rest. Storage supplied by value satisfies both branches at
   once — the caller supplies it and it then lives in the instance — so the two places
   remain the whole inventory. That the inventory is complete is at present recorded only in
   :doc:`open-questions`, a page written to be deleted when its last entry closes, which
   would take the only statement of closure with it.

.. llr:: Every input is supplied by the caller
   :id: UDSS_LLR_0009
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   Every input to the session layer shall be supplied by the caller. The session layer
   shall obtain information about the application and the transport by no other means.
   Creation of the instance precedes every input and is not one.

   Rationale: this excludes every source of state but an input the caller supplies — the
   session layer reading a clock itself, or consulting a global variable, would each be a
   second channel — which is what makes this crate sans-io: there is no way for state to
   reach the session layer except through the inputs the caller gives it.

.. llr:: The inputs the caller supplies
   :id: UDSS_LLR_0010
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The inputs the caller supplies shall include ``S_Data.req``, as ``UDSS_LLR_0021`` defines
   it; ``T_Data.ind``, ``T_DataSOM.ind`` and ``T_Data.conf``, as ``UDSS_LLR_0022`` defines
   them; a timestamp, as ``UDSS_LLR_0020`` defines it, accompanying every other input and
   also supplied on its own; the protocol parameters of ``UDSS_LLR_0040``; the completion
   report of ``UDSS_LLR_0074``; the opening of a channel under ``UDSS_LLR_0121`` and its
   withdrawal under ``UDSS_LLR_0125``; and the channel reset and keep-alive release of
   ``UDSS_LLR_0180`` and ``UDSS_LLR_0184``. The last five — the completion report, the
   opening of a channel, its withdrawal, the channel reset and the keep-alive release —
   and the setting of a protocol parameter, are acts of the caller rather than primitives;
   each shall be accompanied by a timestamp as ``UDSS_LLR_0020`` requires, and
   ``UDSS_LLR_0081`` shall order the expiries that timestamp causes, with their indications,
   before the act. Where a requirement says such an act produces no output, that is said of
   the act alone.

   Rationale: the enumeration is open because a list stated as exhaustive would be wrong
   rather than merely incomplete should a document add an input; the acts are named in it
   so that whether an act carries a timestamp is not left to inference, a reset delivered
   at the instant a timer expires otherwise being read by one implementation as swallowing
   the expiry's indication and by another as following it. A timestamp may be supplied on
   its own because a timer can expire while no message is exchanged, and ``UDSS_LLR_0100``
   requires the server to act on that expiry.

.. llr:: Outputs are retrieved, not pushed
   :id: UDSS_LLR_0011
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   Every output of the session layer shall be produced for the caller to retrieve. The
   session layer shall not invoke a callback, handler, or caller-supplied trait
   implementation in order to deliver an output.

   Rationale: a session layer that calls outwards is one whose behaviour depends on what
   the caller does while the session layer is part-way through a decision. Producing
   outputs for retrieval keeps their ordering explicit and makes reentrancy impossible.
   The prohibition on callbacks is verified by inspection of the crate's public types,
   which take no caller-supplied trait object or function, rather than by a runtime test.

.. llr:: The outputs the session layer produces
   :id: UDSS_LLR_0012
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The outputs the session layer produces for the caller to retrieve shall include
   ``S_Data.ind`` and ``S_Data.conf``, as ``UDSS_LLR_0021`` defines them, and
   ``T_Data.req``, as ``UDSS_LLR_0022`` defines it.

   Rationale: the enumeration is open so that outputs the standard does not define, such
   as the session-timeout indication required by ``UDSS_LLR_0100``, can be delivered by
   the same mechanism as the standard's own primitives.

.. llr:: The session layer retains no message payload
   :id: UDSS_LLR_0013
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not copy or retain the contents of ``S_Data`` or ``T_Data``
   beyond the processing of the input that carried them.

   Rationale: the crate is ``no_std`` and, as ``UDSS_LLR_0004`` requires, allocation-free,
   so it cannot own a buffer whose size it does not know. Retaining a payload would also
   imply a retransmission buffer, and no requirement in this set obliges the session layer
   to retransmit anything. The requirement is verified by inspection of the crate's types,
   which hold no buffer in which a payload could be retained, rather than by a runtime test.

.. llr:: An output refers to caller-owned data
   :id: UDSS_LLR_0014
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   An output that refers to message data shall refer to data owned by the caller.

   Rationale: ``UDSS_LLR_0047`` maps ``S_Data`` onto ``T_Data``, so the two names denote
   the same octets travelling in opposite directions; this requirement binds both. The
   requirement is verified by inspection of the crate's types, which borrow the caller's
   data for the duration of one input rather than copying it, so what an output refers to
   is always the caller's own bytes, rather than by a runtime test.

.. llr:: A rejected input produces no output and changes nothing
   :id: UDSS_LLR_0015
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface

   Where a requirement in this set requires the session layer to reject an input supplied
   by the caller, the session layer shall report the rejection to the caller, shall produce
   for the rejected input itself no output to the application and no output to the transport
   layer, and shall leave its state as the expiries of the accompanying timestamp left it
   under ``UDSS_LLR_0081`` and otherwise unchanged. The indications those expiries produce
   are not outputs of the rejected input and shall be produced, ``UDSS_LLR_0081`` ordering
   them before the rejection report.

   Rationale: requirements throughout this set refuse an input rather than react to it, and
   without this requirement none would say what refusal means. A rejection cannot be
   reported as an ``S_Data.conf``: ``UDSS_LLR_0056`` reserves every ``S_Result`` value
   other than ``S_OK`` for an error detected by a lower layer, and no lower layer is
   involved, no message having been transmitted. Nor is it produced for the caller to
   retrieve on the application's behalf, as an output is; a rejection is addressed to the
   caller that made the erroneous call.

   The expiries are excepted for the reason ``UDSS_LLR_0081`` gives for ordering them ahead
   of the report: the elapsed time preceded the input's arrival, so the timer expired before
   the session layer saw what the caller supplied. Suppressing their indications would let a
   malformed input swallow an expiry the timestamp had already caused — the session-timeout
   indication of ``UDSS_LLR_0100`` among them — which is the failure ``UDSS_LLR_0081``
   exists to prevent.

.. llr:: What a rejection report carries
   :id: UDSS_LLR_0016
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface

   A rejection report shall state the cause of the rejection and, where the rejecting
   requirement states content for the report, that content; where several requirements
   reject the same input, the one report shall state every cause and carry the content
   each of them requires.

   Rationale: the report carries content because ``UDSS_LLR_0172`` has it state the time
   remaining before a postponed request may be sent and ``UDSS_LLR_0179`` the cause or
   causes of a refused repeat.

Timebase
--------

The session layer's whole notion of time is the caller's. These requirements fix what a
timestamp is and where it comes from; :doc:`llr-timer-model` fixes what a timer does with
one.

.. llr:: The session layer reads no clock
   :id: UDSS_LLR_0017
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io; timing

   The session layer shall not read a clock. Every decision that depends on elapsed time
   shall be made from a timestamp supplied by the caller.

   Rationale: reading a clock is I/O by another name, and it makes timer behaviour
   untestable except in real time. A caller-supplied timestamp lets a test advance time
   arbitrarily, and lets each deployment choose the time source its platform provides.

.. llr:: A timestamp is a 32-bit unsigned count of milliseconds
   :id: UDSS_LLR_0018
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   A timestamp shall be a 32-bit unsigned count of milliseconds.

   Rationale: the unit is milliseconds because that is the unit ISO 14229-2:2021 9.5
   Table 5 states its timing parameter values in; ``tS3_Server`` has a timeout of 5 000 ms
   and a tolerance of 0 ms to 200 ms. The width is fixed rather than left as a minimum
   because the wraparound point would otherwise be unknown, and an unknown modulus cannot
   be tested.

.. llr:: An interval is the modular difference of two timestamps
   :id: UDSS_LLR_0019
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   The session layer shall compute the interval between two timestamps as their difference
   modulo 2\ :sup:`32`, and shall treat that result as the elapsed time between them.

   Rationale: the subtraction is total, so no input can leave the session layer without a
   defined elapsed time, and modular subtraction returns the true interval for any interval
   shorter than the wrap, which every timeout in this set is by orders of magnitude.
   Whether the caller's timestamps are non-decreasing is a property of the caller, recorded
   as an assumption of use in the qualification repository; a caller that supplies a
   decreasing timestamp obtains an interval close to the full range, which will expire
   timers early, and the session layer cannot distinguish that from a legitimate wrap.

.. llr:: Every input is accompanied by a timestamp
   :id: UDSS_LLR_0020
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   Every input shall be accompanied by a timestamp, which is the time at which the session
   layer treats that input as having occurred. A timestamp may also be supplied with no
   other input.

   Rationale: every timer requirement in this set starts, stops or expires a timer on an
   input, and an input with no time attached would have to be placed at the time of the
   last one, an interval the caller controls and the set does not state. A timestamp may be
   supplied on its own because a timer can expire while no message is exchanged.

Service primitives
------------------

.. llr:: The service interface comprises three service primitives
   :id: UDSS_LLR_0021
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.1
   :tags: service-interface; primitives

   The session layer shall provide three service primitives to the application:

   * ``S_Data.req``, an input, by which the application passes control information or data
     to be transmitted to the session layer;
   * ``S_Data.ind``, an output, by which the session layer passes status information and
     received data to the application;
   * ``S_Data.conf``, an output, by which the session layer passes to the application the
     status of a preceding ``S_Data.req``.

.. llr:: The session layer exchanges four protocol data units with the transport layer
   :id: UDSS_LLR_0022
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.3; ISO 14229-2:2021 7.3; ISO 14229-2:2021 9.2 Table 3
   :tags: service-interface; primitives

   The session layer shall exchange the following protocol data units with the transport
   layer:

   * ``T_Data.req``, an output, requesting transmission of a message;
   * ``T_Data.ind``, an input, reporting the reception of a complete message;
   * ``T_DataSOM.ind``, an input, reporting the start of reception of a multi-frame
     message;
   * ``T_Data.conf``, an input, reporting the outcome of a requested transmission.

   Each locator supplies a different part of this set. Clause 6.3 names ``T_Data.ind`` and
   ``T_DataSOM.ind``; clause 7.3 names ``T_Data.conf`` and establishes that a transmission
   request is passed to the transport layer; clause 9.2 Table 3 names that request
   ``T_Data.req``, in defining ``tP4_Server`` as the time between a ``T_Data.ind`` and the
   ``T_Data.req`` that starts the final response.

.. llr:: T_DataSOM.ind carries addressing and no result
   :id: UDSS_LLR_0023
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; primitives

   ``T_DataSOM.ind`` shall carry ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``
   and, where ``S_Mtype`` requires it, ``S_AI[AE]``. It shall carry no data, length or
   result.

   Rationale: which parameters ``T_DataSOM.ind`` carries is stated here because the
   standard nowhere enumerates them. Clause 7.3 keeps the indication inside the session
   layer and defines no mapping for it onto an S_PDU; its Table 2 lists the transport
   parameters a message carries without saying which of them the start-of-message reports;
   and 9.1.2 REQ 5.13 speaks of "the parameters included in either ``T_DataSOM.ind`` or the
   ``T_Data.ind`` service primitive" without saying what they are. The
   addressing is what ``UDSS_LLR_0045``'s pairing needs. A result is excluded because a
   start-of-message reports a reception that has neither succeeded nor failed; the outcome
   is reported by the completion, and every requirement in this set that acts on a failed
   reception acts on a ``T_Data.ind``.

.. llr:: T_Data.req carries the request's parameters
   :id: UDSS_LLR_0024
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3; ISO 14229-2:2021 9.2 Table 3
   :tags: service-interface; primitives

   ``T_Data.req`` shall carry every parameter of the ``S_Data.req`` it is produced from,
   mapped as ``UDSS_LLR_0047`` requires.

.. llr:: T_Data.conf carries addressing and a result
   :id: UDSS_LLR_0025
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.6; ISO 14229-2:2021 7.3
   :tags: service-interface; primitives

   ``T_Data.conf`` shall carry ``T_Ptype``, ``T_AI[TAtype]``, ``T_AI[SA]``, ``T_AI[TA]``,
   ``T_AI[AE]`` where ``T_Ptype`` requires it, and ``T_Result``, mapped onto the session
   layer's parameters as ``UDSS_LLR_0047`` requires; it shall carry no data and no length.

   ``T_Data.conf``'s parameters are stated because the standard states them only by mapping:
   clause 7.6 has the ``S_Data.conf`` identify the ``S_Data.req`` it confirms by the address
   information and report ``S_Result``, and 7.3 Table 2 maps each of those onto the
   transport parameter of the same name, so the confirmation the transport delivers carries
   the addressing and the result and nothing else.

.. llr:: The caller identifies the channel of every inbound indication at a client
   :id: UDSS_LLR_0026
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; channel

   At a client, every ``T_DataSOM.ind`` and ``T_Data.ind`` shall identify the logical
   communication channel it belongs to, supplied by the caller; ``UDSS_LLR_0121`` defines
   the channel.

   Rationale: the caller identifies the channel because the session layer cannot. A server
   answers the one client that asked, so every response it sends is physically addressed
   to the client whether the request that provoked it was physical or functional, an
   observation this set relies on rather than one the standard states; a response from one
   server may belong to the physical channel to that server or to a functional channel it
   was reached through, and nothing in the indication says which. The caller that issued
   the request knows.

.. llr:: An indication naming no channel, or no existing one, is rejected
   :id: UDSS_LLR_0027
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; channel

   An indication identifying a channel the client does not have, or identifying no channel
   where ``UDSS_LLR_0026`` requires one, shall be rejected as ``UDSS_LLR_0015`` defines. An
   interface in which the identifier cannot be omitted satisfies the second limb without a
   check.

   Rationale: naming a channel that does not exist is a caller error, not an input, and so
   is omitting the identifier ``UDSS_LLR_0026`` requires. The omission is named here because
   ``UDSS_LLR_0072`` reaches only the classification and addressing forms, of which the
   channel identifier is none, so without this limb ``UDSS_LLR_0026`` would be an obligation
   on the caller with no stated outcome — what ``UDSS_LLR_0070``'s rationale calls an
   obligation no test could check.

.. llr:: The identified channel is not checked against the indication's addressing
   :id: UDSS_LLR_0028
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; channel

   The session layer shall not verify the channel identified under ``UDSS_LLR_0026``
   against the indication's addressing.

   Rationale: the channel named is trusted rather than checked because on a functional
   channel the response's addressing does not name the channel, and a check on a physical
   channel alone would catch a misrouting only by coincidence; ``UDSS_LLR_0126`` records
   the residual.

.. llr:: An instance has one role, fixed at creation
   :id: UDSS_LLR_0029
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; role

   An instance of the session layer shall be created as a client or as a server, and its
   role shall not change thereafter.

   Rationale: every requirement in this set is stated for the client or for the server, and
   ISO 14229-2:2021 describes the two as distinct peer entities throughout clauses 6 to 10,
   each with its own timers in 9.6 Tables 7 and 8. Nothing in the set said how an instance
   came to be one or the other, so an implementer could build one instance that plays both
   roles and another that must be told, with the requirements silent on inputs that belong
   to the other role. A node that is both, a gateway or a tester under test, is two
   instances. The role is fixed at creation because no requirement gives a role change a
   meaning, and state held for one role has none in the other.

.. llr:: The inputs a server rejects
   :id: UDSS_LLR_0030
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; role

   A server shall reject, as ``UDSS_LLR_0015`` defines:

   * an ``S_Data.req`` whose classification states kind ``request``;
   * a ``T_Data.ind`` or ``T_DataSOM.ind`` whose classification states kind ``final
     response`` or ``response pending``;
   * a ``T_DataSOM.ind`` or ``T_Data.ind`` identifying a logical communication channel;
   * the opening of a channel under ``UDSS_LLR_0121`` and its withdrawal under
     ``UDSS_LLR_0125``;
   * the channel reset of ``UDSS_LLR_0180``;
   * the keep-alive release of ``UDSS_LLR_0184``.

   An interface in which a server cannot be handed a request classification to transmit, a
   response classification to receive, a channel identifier, the opening or withdrawal of
   a channel, a channel reset or a keep-alive release satisfies this requirement without a
   check.

   Rationale: the rejected inputs are listed rather than described, because "an input
   whose form belongs to the other role" is not decidable for an ``S_Data.req``, whose
   form ``UDSS_LLR_0065`` defines without a role: a server asked to transmit a request
   would otherwise be accepted by one implementation and refused by another. The
   reception primitives are listed for the same reason: no server requirement conditions on
   receiving a response, so one implementation would forward such an indication under
   ``UDSS_LLR_0036`` and another refuse it. The remaining inputs — an indication
   identifying a channel, and the opening or withdrawal of a channel, the channel reset
   and the keep-alive release that only a client performs — belong to the client alone;
   they are listed here rather than left to each owning document because a rule split
   between the two places was honoured by neither.

.. llr:: The inputs a client rejects
   :id: UDSS_LLR_0031
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; role

   A client shall reject, as ``UDSS_LLR_0015`` defines:

   * an ``S_Data.req`` whose classification states kind ``final response`` or ``response
     pending``;
   * the completion report of ``UDSS_LLR_0074``;
   * a ``T_Data.ind`` or ``T_DataSOM.ind`` whose classification states kind ``request``.

   An interface in which a client cannot be handed a response classification to transmit, a
   request classification to receive, or a completion report satisfies this requirement
   without a check.

   Rationale: the rejected inputs are listed rather than described, because "an input
   whose form belongs to the other role" is not decidable for an ``S_Data.req`` or the
   completion report, whose forms ``UDSS_LLR_0065`` and ``UDSS_LLR_0074`` define without a
   role: a client told a request it never received is complete would otherwise be
   accepted by one implementation and refused by another. The reception primitives are
   listed for the same reason: no client requirement conditions on receiving a request,
   so one implementation would forward such an indication under ``UDSS_LLR_0036`` and
   another refuse it. These inputs are listed here rather than left to each owning
   document because a rule split between the two places was honoured by neither.

.. llr:: What creation supplies
   :id: UDSS_LLR_0032
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; role

   Creation of a server shall supply the association storage of ``UDSS_LLR_0059`` and the
   ``tS3_Server``, ``tP2_Server_Max`` and ``tP2*_Server_Max`` parameters of
   ``UDSS_LLR_0042``. Creation of a client shall supply the keep-alive mode of
   ``UDSS_LLR_0149`` and, in functional keep-alive, the storage of ``UDSS_LLR_0150`` and
   reload parameter of ``UDSS_LLR_0152``, together with the client's channel storage — the
   physical and functional arrays ``UDSS_LLR_0126`` holds a channel's facts in, including
   the one association it may carry. Opening a channel, under ``UDSS_LLR_0121``, shall
   supply that channel's own parameters: the response-window pair of ``UDSS_LLR_0132``,
   the spacing parameter of ``UDSS_LLR_0165``, and, on a physical channel, the
   ``tS3_Client`` reload of ``UDSS_LLR_0152``.

   Rationale: what creation supplies is gathered here because it was stated in four
   places and enumerated in none, and a tester building the first test must collect it.

.. llr:: S_Data.req requests transmission of a message
   :id: UDSS_LLR_0033
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.4
   :tags: service-interface; primitives

   The session layer shall accept from the application an ``S_Data.req`` carrying
   ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``, ``S_AI[AE]`` where
   ``S_Mtype`` requires it, ``S_Data``, and ``S_Length``.

   On ``S_Data.req`` the session layer shall request transmission of ``S_Length`` bytes of
   ``S_Data`` to the peer entity identified by the addressing parameters, and shall
   subsequently report the completion or failure of that transmission by an ``S_Data.conf``.

.. llr:: S_Data.ind delivers a received message to the application
   :id: UDSS_LLR_0034
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.5
   :tags: service-interface; primitives

   The session layer shall deliver a received message to the application by an
   ``S_Data.ind`` carrying ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``,
   ``S_AI[AE]`` where ``S_Mtype`` requires it, ``S_Data``, ``S_Length``, and ``S_Result``.
   The addressing parameters shall identify the peer entity from which the message was
   received.

.. llr:: S_Data and S_Length are valid only on a successful reception
   :id: UDSS_LLR_0035
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.5
   :tags: service-interface; primitives

   On ``S_Data.ind``, ``S_Data`` and ``S_Length`` shall be valid only where ``S_Result``
   is ``S_OK``.

   An interface that carries the data and the result as one value, in which the data is
   present only where the result is ``S_OK``, satisfies this without a check.

   The same shape serves the ``T_Data.ind`` that ``UDSS_LLR_0047`` maps onto this
   indication: a reception input carrying its data and its result as one value cannot
   supply data that this requirement then makes invalid. An interface that carries them as
   separate parameters satisfies this requirement too, by stating that the data is valid
   only where the result is ``S_OK``: the data a failed reception supplies is passed to the
   ``S_Data.ind`` as it came and is read by nothing. Neither shape is a further obligation:
   nothing here requires the data of a failed reception, and nothing may use it.

.. llr:: A received message is indicated to the application
   :id: UDSS_LLR_0036
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3; ISO 14229-2:2021 7.5; ISO 14229-2:2021 8.10
   :tags: service-interface; primitives

   On ``T_Data.ind``, whether it reports a successful or an unsuccessful reception, the
   session layer shall produce an ``S_Data.ind`` whose parameters are those of the
   received T_PDU mapped as ``UDSS_LLR_0047`` requires.

   Both outcomes produce an indication because ``S_Result`` exists to report them:
   ``UDSS_LLR_0035`` makes ``S_Data`` and ``S_Length`` valid only where ``S_Result`` is
   ``S_OK``, which presupposes indications where it is not, and a client cannot detect a
   failed reception that is never indicated to it. Clause 8.10 requires the error result to
   be issued to the service user on the receiver side as on the sender side, and ISO
   14229-2:2021 9.7 Table 9 obliges the client to repeat the last request when a response
   reception fails, which it cannot do unshown. Table 10 obliges the server to restart
   ``tS3_Server`` and otherwise only to ignore such a request, which ``UDSS_LLR_0092``
   transcribes and reads as acting on nothing further rather than as withholding the
   indication. No requirement in the set withholds an indication. A ``T_Data.ind`` rejected
   under ``UDSS_LLR_0027``, ``UDSS_LLR_0030``, ``UDSS_LLR_0031``, ``UDSS_LLR_0069``,
   ``UDSS_LLR_0071`` or ``UDSS_LLR_0072`` is not withheld but refused: ``UDSS_LLR_0015``
   governs it and this requirement does not reach it.

.. llr:: S_Data.conf confirms a preceding S_Data.req
   :id: UDSS_LLR_0037
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.6
   :tags: service-interface; primitives

   The session layer shall confirm the completion of an ``S_Data.req`` by an ``S_Data.conf``
   carrying ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``, ``S_AI[AE]`` where
   ``S_Mtype`` requires it, and ``S_Result``. The addressing parameters shall identify the
   ``S_Data.req`` being confirmed, and ``S_Result`` shall report its outcome.

.. llr:: T_DataSOM.ind is not forwarded to the application
   :id: UDSS_LLR_0038
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3
   :tags: service-interface; primitives

   ``T_DataSOM.ind`` shall not be mapped onto any S_PDU. On ``T_DataSOM.ind`` the session
   layer shall produce no ``S_Data.ind``.

   The indication is used only within the session layer, to perform session layer timing.
   The prohibition is on the ``S_Data.ind``, not on every output: a timer requirement that
   conditions on the indication may deliver an indication of its own, as the expiry
   indications do on any input.

.. llr:: T_Data.conf is forwarded to the application
   :id: UDSS_LLR_0039
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3; ISO 14229-2:2021 7.4
   :tags: service-interface; primitives

   On ``T_Data.conf`` reporting the outcome of a transmission previously requested
   through an ``S_Data.req``, the session layer shall produce an ``S_Data.conf``
   reporting that outcome to the application.

   The application needs the confirmation in order to start actions that are executed
   immediately after transmission of a request or response message, such as an ECU reset
   or a bit rate change.

.. llr:: Protocol parameters are set through the service interface
   :id: UDSS_LLR_0040
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.1
   :tags: service-interface; primitives

   The session layer shall provide for the setting of its protocol parameters by the
   caller.

   Clause 6.1 places the setting of protocol parameters in the service interface alongside
   transmission and reception.

.. llr:: Every timing parameter is a 32-bit value in the timestamp's unit
   :id: UDSS_LLR_0041
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   Every timing parameter that a requirement in this set conditions on, including the
   ``tS3_Server`` timeout that ``UDSS_LLR_0100`` compares elapsed time against, shall be
   supplied as a protocol parameter and shall be expressed in the unit ``UDSS_LLR_0018``
   gives for a timestamp, as a 32-bit unsigned value.

   Rationale: the width matches the timestamp's because an interval is a modular
   difference of timestamps and a value beyond that range could never be reached.

.. llr:: A parameter a timer loads has a fixed supply point and no default
   :id: UDSS_LLR_0042
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   Every protocol parameter that a requirement loads a timer with shall be supplied with
   the instance it belongs to when that instance is created, with the caller-supplied
   storage it belongs to when that storage is created, or with the channel it belongs to
   when that channel is opened, and shall have no default.

   Rationale: no requirement in this set fixes a value for any timing parameter: the
   recommended and default values in ISO 14229-2:2021 9 are properties of a vehicle
   network and a deployment, not of this crate. That is also why a parameter has no
   default and is supplied at creation: a timer started before its parameter existed
   would have to be loaded with a value the set declines to choose.

.. llr:: A parameter may be set again at any time
   :id: UDSS_LLR_0043
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; timing

   A protocol parameter may be set again at any time, and a change shall affect only a
   timer set running after it; a timer already running keeps the value it was loaded
   with, under ``UDSS_LLR_0076``.

   Rationale: a caller correcting a parameter must not disturb a window a timer already
   running holds a peer to. ``UDSS_LLR_0076`` is the mechanism that leaves that value
   alone; this requirement states the outcome it serves.

Message and peer identity
--------------------------

Two definitions the whole set depends on: which two addressing identities are the same
peer, and how a multi-frame message's start is matched to its completion.

.. llr:: Peer identity and its equality
   :id: UDSS_LLR_0044
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; identity

   A **peer identity** shall be formed by an address and, where ``S_Mtype`` carries one,
   an address extension. Two peer identities shall be equal only where both carry an
   extension and the addresses and the extensions are equal, or neither carries one and the
   addresses are equal; an identity that carries an extension is never equal to one that
   does not.

   Rationale: ``UDSS_LLR_0045`` identifies a responder by this pair, ``UDSS_LLR_0082``
   records the controlling client as one, ``UDSS_LLR_0104`` records the service in
   progress as one, and ``UDSS_LLR_0139`` keys the responder table on one. Stated once,
   the four cannot drift apart. The extension is part of the identity because two clients
   behind one remote address can differ in it. Matching a confirmation's ``S_AI[TA]`` and
   ``S_AI[AE]`` against a recorded ``S_AI[SA]`` and ``S_AI[AE]`` reads the extension as the
   same value on a response as on the request it answers; ISO 14229-2:2021 8.7 says only
   that ``S_AE`` carries the node's extended address, the symmetry being the network
   layer's, and the reading is recorded here.

.. llr:: First indication, completion and the start-of-message pairing
   :id: UDSS_LLR_0045
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.3; ISO 14229-2:2021 7.3; ISO 14229-2:2021 9.2 Table 3
   :tags: service-interface; primitives; identity

   A ``T_DataSOM.ind`` shall open a start-of-message on its channel for the **responder**
   identified by its ``S_AI[SA]`` and, where ``S_Mtype`` carries one, its ``S_AI[AE]``,
   as ``UDSS_LLR_0044`` forms and equates an identity, leaving one already open for that
   responder as it is. A ``T_Data.ind`` shall **complete** the open start-of-message on the
   same channel from the same responder where one exists, closing it, and shall otherwise
   report a **single-frame** message.

   Throughout this set, the **first indication** of a message is its ``T_DataSOM.ind``, or a
   ``T_Data.ind`` that completes no start-of-message; a **completion** is a ``T_Data.ind``
   that completes one.

   Table 3 makes single-frame against multi-frame the transport's distinction, and on a
   functional channel the multi-frame responses of several servers may interleave, so
   matching has to name the responder. The state this costs is stated with the client's
   requirements, in ``UDSS_LLR_0139`` and ``UDSS_LLR_0126``; the server needs none of it.

Parameter mapping
-----------------

.. llr:: Parameter validity in each service primitive
   :id: UDSS_LLR_0046
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.2 Table 1
   :tags: service-interface; mapping

   The validity of each session layer parameter in each service primitive shall be as
   follows. The application layer column records the parameter each one corresponds to in
   ISO 14229-2:2021 Table 1; the correspondence is by name and imposes nothing further on
   this crate, to which the caller supplies ``S_`` parameters directly.

   Table 1's validity columns are headed with the application layer's ``A_Data`` primitive
   names while the clause containing it is titled for the session layer's ``S_Data``
   primitives. The two are read here as the same three primitives under two layers' names,
   which is what the parameter correspondence in the same table implies.

   .. list-table::
      :header-rows: 1
      :widths: 24 24 17 17 18

      * - Application layer
        - Session layer
        - ``.req``
        - ``.ind``
        - ``.conf``
      * - ``A_Mtype``
        - ``S_Mtype``
        - valid
        - valid
        - valid
      * - ``A_AI[TAtype]``
        - ``S_AI[TAtype]``
        - valid
        - valid
        - valid
      * - ``A_AI[TA]``
        - ``S_AI[TA]``
        - valid
        - valid
        - valid
      * - ``A_AI[SA]``
        - ``S_AI[SA]``
        - valid
        - valid
        - valid
      * - ``A_AI[AE]``
        - ``S_AI[AE]``
        - valid
        - valid
        - valid
      * - ``A_Length``
        - ``S_Length``
        - valid
        - valid
        - not applicable
      * - ``A_Data``
        - ``S_Data``
        - valid
        - valid
        - not applicable
      * - ``A_Result``
        - ``S_Result``
        - not applicable
        - valid
        - valid

   ``S_AI[AE]`` is subject to a further condition that this table does not state. Table 1
   marks it valid in all three primitives unconditionally, while ISO 14229-2:2021 8.3
   includes it in the address information only for the remote message types. Per
   ``UDSS_LLR_0048`` and ``UDSS_LLR_0052`` it is present only where ``S_Mtype`` is
   ``RDiag`` or ``SecureRDiag``.

.. llr:: Session layer parameters map onto transport layer parameters
   :id: UDSS_LLR_0047
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3 Table 2
   :tags: service-interface; mapping

   The session layer shall map the parameters of its protocol data unit onto the
   parameters of the transport layer protocol data unit, and the reverse, as follows.

   .. list-table::
      :header-rows: 1
      :widths: 30 30 40

      * - S_PDU parameter
        - T_PDU parameter
        - Description
      * - ``S_Mtype``
        - ``T_Ptype``
        - Session layer message type, transport layer segment type
      * - ``S_AI[TAtype]``
        - ``T_AI[TAtype]``
        - Target address type
      * - ``S_AI[SA]``
        - ``T_AI[SA]``
        - Source address
      * - ``S_AI[TA]``
        - ``T_AI[TA]``
        - Target address
      * - ``S_AI[AE]``
        - ``T_AI[AE]``
        - Address extension
      * - ``S_Data``
        - ``T_Data``
        - Message data
      * - ``S_Length``
        - ``T_Length``
        - Length of the message data
      * - ``S_Result``
        - ``T_Result``
        - Result of the service execution

Service primitive parameters
----------------------------

The requirements below constrain each parameter's value set and width. ISO 14229-2:2021
8.2 defines the data types they are named in terms of: ``Enum`` is an 8-bit enumeration,
``Unsigned Word`` a 16-bit unsigned value, ``Unsigned Long`` a 32-bit unsigned value, and
``Byte Array`` a sequence of 8-bit aligned data. Where a requirement below gives a width,
that width is normative. Where it gives only a value set, as the enumerations do, the
in-memory representation is an implementation choice.

That is a deliberate departure from 8.2 for the three enumerated parameters,
``UDSS_LLR_0048``, ``UDSS_LLR_0049`` and ``UDSS_LLR_0056``. The clause makes ``Enum`` an
8-bit type; this crate neither encodes nor decodes those parameters on the wire, so their
width constrains nothing observable, and fixing it would forbid a Rust representation that
is safer and no larger. The value sets are transcribed exactly.

.. llr:: S_Mtype identifies the message type and the address information present
   :id: UDSS_LLR_0048
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.3
   :tags: service-interface; parameters

   ``S_Mtype`` shall be an enumeration whose values are ``Diag``, ``RDiag``, ``SecureDiag``
   and ``SecureRDiag``.

   Where ``S_Mtype`` is ``Diag`` or ``SecureDiag``, the address information shall consist of
   ``S_SA``, ``S_TA`` and ``S_TAtype``. Where ``S_Mtype`` is ``RDiag`` or ``SecureRDiag``,
   the address information shall additionally include ``S_AE``.

.. llr:: S_TAtype selects the communication model
   :id: UDSS_LLR_0049
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.4
   :tags: service-interface; parameters

   ``S_TAtype`` shall be an enumeration whose values are ``physical`` and ``functional``.
   ``physical`` shall denote one-to-one communication with a single peer entity, and
   ``functional`` shall denote one-to-many communication.

.. llr:: S_TA carries the target address
   :id: UDSS_LLR_0050
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.5
   :tags: service-interface; parameters

   ``S_TA`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall encode the receiving session layer protocol entity.

.. llr:: S_SA carries the source address
   :id: UDSS_LLR_0051
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.6
   :tags: service-interface; parameters

   ``S_SA`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall encode the sending session layer protocol entity.

.. llr:: S_AE carries the address extension
   :id: UDSS_LLR_0052
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.7; ISO 14229-2:2021 8.3
   :tags: service-interface; parameters

   ``S_AE`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall carry the extended address of the node. It shall be present only where ``S_Mtype``
   is ``RDiag`` or ``SecureRDiag``.

.. llr:: S_Length carries the length of S_Data
   :id: UDSS_LLR_0053
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.8
   :tags: service-interface; parameters

   ``S_Length`` shall be a 32-bit unsigned value in the range ``0x00000000`` to
   ``0xFFFFFFFF``, and shall carry the number of bytes of ``S_Data`` to be transmitted or
   received.

.. llr:: A length differing from the data supplied is rejected
   :id: UDSS_LLR_0054
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; parameters

   On ``S_Data.req``, an ``S_Length`` differing from the number of bytes of ``S_Data``
   supplied shall be rejected as ``UDSS_LLR_0015`` defines; an interface that carries the
   two as one value satisfies this without a check.

   Rationale: the rejection is the set's own, not clause 8.8's, which defines the
   parameter and says nothing of a mismatch. Without it ``UDSS_LLR_0033``'s "``S_Length``
   bytes of ``S_Data``" would be satisfiable two ways by a caller that supplied them
   separately.

.. llr:: S_Data carries the message data
   :id: UDSS_LLR_0055
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.9
   :tags: service-interface; parameters

   ``S_Data`` shall be a sequence of 8-bit values and shall carry the message data
   content of the request or response message to be transmitted or received.

.. llr:: S_Result reports the outcome of a service execution
   :id: UDSS_LLR_0056
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.10
   :tags: service-interface; parameters

   ``S_Result`` shall be an enumeration reporting the outcome of a service execution. The
   value ``S_OK`` shall indicate that the service execution completed successfully. Every
   other value shall indicate an error detected by a lower layer. ``S_Result`` shall be
   reported to the application on both the sending and the receiving side.

   ISO 14229-2:2021 does not enumerate the error values. Clause 8.10 states only that an
   error value is issued when an error is detected by a lower layer, which is why this set
   has the session layer carry such a value without interpreting it rather than act on its
   meaning; that carrying is this set's decision and is not stated by the clause. That
   clause also requires the application layer entity to set the appropriate error bit where
   two or more errors are discovered at once; that obligation falls on the application layer
   and is not transcribed here, and no requirement in this set depends on ``S_Result`` being
   a bit field rather than an enumeration.

Message classification
----------------------

Several requirements in this set condition on what a message is rather than on its
addressing alone. The session layer does not determine that by parsing. These requirements
are derived: ISO 14229-2 states the conditions in terms of message content and leaves the
means of recognising it to the implementation.

.. llr:: Message classification is supplied by the caller
   :id: UDSS_LLR_0057
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   ``S_Data.req``, ``T_Data.ind`` and ``T_DataSOM.ind`` shall each carry a message
   classification supplied by the caller. Where a requirement in this set depends on what
   a message is, the session layer shall determine it from that classification.

   Rationale: several requirements condition on message content rather than on addressing
   alone: on a message's kind, on whether it selects a diagnostic session, on whether a
   response was solicited, and on whether a request is a keep-alive or a repeat. Determining
   those facts by parsing ``S_Data`` would bind this crate to the ISO 14229-1 application
   layer encodings and would require every timing test to construct valid UDS frames. The
   caller already holds what is needed: the application composes the message it asks to have
   transmitted, and the code that supplies a ``T_Data.ind`` holds the bytes it received.

   The classification is carried on ``T_Data.ind`` rather than on ``S_Data.ind`` because a
   client must recognise a response-pending response at reception, before the application
   has seen it. It is carried on ``T_DataSOM.ind`` for the same reason in the other
   direction: ``UDSS_LLR_0087`` conditions on a start-of-message that begins a request. A
   caller supplying a start-of-message indication holds its first frame, so the
   classification is available there.

.. llr:: The kind required on a failed reception addressed to a server
   :id: UDSS_LLR_0058
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   Where a ``T_Data.ind`` reports an unsuccessful reception, the message classification it
   carries under ``UDSS_LLR_0057`` shall state kind ``request`` where the message was
   addressed to a server, and may otherwise omit the kind.

   Rationale: ``UDSS_LLR_0092`` conditions on a failed reception of a request, so the kind
   is required in that case even where the message data is incomplete. It is omissible
   otherwise because a client whose transport reports a broken reception may be unable to
   tell a final response from a response-pending one, and ``UDSS_LLR_0065`` offers no value
   for a kind that was not determinable.

.. llr:: The classification is associated with the transmission it describes
   :id: UDSS_LLR_0059
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall associate the classification carried by an ``S_Data.req`` with
   the ``T_Data.req`` produced from it and with the ``T_Data.conf`` that reports the
   outcome of that transmission. It shall hold that association, from the ``S_Data.req``
   until the ``T_Data.conf``, in storage supplied by the caller, together with the
   addressing parameters of the ``S_Data.req``, and shall match a ``T_Data.conf`` to the
   outstanding association whose ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``
   and, where ``S_Mtype`` carries one, ``S_AI[AE]`` equal the confirmation's.

   Rationale: the association is matched on addressing because that is the standard's own
   rule: ISO 14229-2:2021 7.6 has the ``S_Data.conf`` confirm "the completion of an
   S_Data.req service identified by the address information", and 7.3 Table 2 maps the
   transport's confirmation onto the same parameters, so nothing else travels on a
   confirmation that could identify the request it answers.

   The match is a declared widening of that list in one parameter: 7.6 identifies the
   ``S_Data.req`` by ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]`` and ``S_AI[AE]`` alone,
   and this requirement matches on ``S_Mtype`` as well. It is deliberate, and it is
   available: 7.3 Table 2 maps ``S_Mtype`` onto the transport's ``T_Ptype``, so the
   confirmation carries it. Two transmissions to one peer differing only in ``S_Mtype``, a
   ``Diag`` and a ``SecureDiag`` message, differ in addressing under ``UDSS_LLR_0060`` and
   so may be outstanding together; matching on 7.6's list alone would let each confirmation
   match the other's association, and the classification read from it would then describe
   the wrong message. ``S_Mtype`` also decides whether an address extension is present at
   all, under ``UDSS_LLR_0048`` and ``UDSS_LLR_0052``, so the condition this rule places on
   ``S_AI[AE]`` is well defined only where the two ``S_Mtype`` agree.

   The storage is the caller's because the number of peers an instance addresses is a
   property of the deployment and the crate does not allocate, as ``UDSS_LLR_0004``
   requires; how the client's storage is organised per channel is ``UDSS_LLR_0126``'s.

   The association with ``T_Data.conf`` is stated because ``UDSS_LLR_0085``,
   ``UDSS_LLR_0088``, ``UDSS_LLR_0090``, ``UDSS_LLR_0091``, ``UDSS_LLR_0093`` and
   ``UDSS_LLR_0094`` all condition on what kind of message a confirmation confirms, and no
   classification travels on the confirmation itself. The association with ``T_Data.req`` is
   stated because ``UDSS_LLR_0114`` conditions on what kind of message a transmission
   request carries.

.. llr:: At most one association is outstanding per addressing
   :id: UDSS_LLR_0060
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   At most one association of ``UDSS_LLR_0059`` between an ``S_Data.req`` and its
   ``T_Data.conf`` shall be outstanding for any one addressing, an addressing being the
   ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]`` and, where ``S_Mtype`` carries
   one, ``S_AI[AE]`` that requirement matches a confirmation on.

   Rationale: matching a confirmation on addressing alone, as ``UDSS_LLR_0059`` requires,
   can only work while one transmission to a given addressing is outstanding, which the
   standard's models guarantee at the client, one request per logical communication
   channel, and assume without saying at the server. Stating the limit, and rejecting under
   ``UDSS_LLR_0061`` and ``UDSS_LLR_0063`` what exceeds it, makes the rule checkable where
   the standard is silent: a server whose application asks to transmit a second response to
   a client while the first is unconfirmed, a periodic transmission alongside a solicited
   one, for instance, would otherwise leave the session layer unable to tell which
   confirmation carried the session selection or the solicitation the timer requirements
   read.

   The limit is survivable because each role has an exit, or an assumption in place of one.
   An association whose ``T_Data.conf`` never arrives stays outstanding: the client's exit
   is the withdrawal of the channel under ``UDSS_LLR_0125``, and the server has none,
   resting instead on the assumption of use that the transport reports a ``T_Data.conf``
   for every ``T_Data.req``, which :doc:`open-questions` records beside the start-of-message
   assumption.

.. llr:: A request duplicating an outstanding association is rejected
   :id: UDSS_LLR_0061
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   An ``S_Data.req`` whose addressing equals that of an association outstanding under
   ``UDSS_LLR_0059`` shall be rejected as ``UDSS_LLR_0015`` defines.

   Rationale: this is what makes the limit of ``UDSS_LLR_0060`` checkable rather than an
   obligation on the caller with no stated outcome.

.. llr:: A request for which no association is free is rejected
   :id: UDSS_LLR_0062
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   An ``S_Data.req`` for which the caller-supplied storage of ``UDSS_LLR_0059`` has no free
   association shall be rejected as ``UDSS_LLR_0015`` defines.

   Rationale: that storage is the caller's and is sized by the deployment, so it can be
   exhausted by a caller addressing more peers than it provided for.

.. llr:: A confirmation matching no outstanding association is rejected
   :id: UDSS_LLR_0063
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A ``T_Data.conf`` matching no association outstanding under ``UDSS_LLR_0059`` shall be
   rejected as ``UDSS_LLR_0015`` defines.

   Rationale: no classification travels on a confirmation itself; it travels with the
   association ``UDSS_LLR_0059`` holds. A confirmation that matches none is therefore a
   confirmation of a transmission this set has no record of, and nothing in the set can
   tell what it confirms. Rejecting it is the only handling that does not guess.

.. llr:: No association is outstanding on initialisation
   :id: UDSS_LLR_0064
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   On initialisation no association of ``UDSS_LLR_0059`` shall be outstanding.

   Rationale: an instance that began with an association outstanding would reject the first
   ``S_Data.req`` to that addressing under ``UDSS_LLR_0061``, or match the first
   ``T_Data.conf`` to a transmission that was never made.

.. llr:: Message classification values
   :id: UDSS_LLR_0065
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A message classification shall consist of a kind and, where the message effects a
   transition to a diagnostic session, a session selection. The kind shall be one of:

   * ``request``, a message sent by a client to a server. On ``S_Data.req``, and on the
     ``T_Data.conf`` that ``UDSS_LLR_0059`` associates with it, a request classification
     shall further state the number of responses expected: ``none``, an exact number of at
     least one, or ``unknown``, and may further state either ``keep-alive``, that the message
     is a TesterPresent the application transmits to keep a non-default session alive, or
     ``repeat``, that the message repeats a request, other than the keep-alive TesterPresent,
     whose transmission, reception or response window failed, as ISO 14229-2:2021 9.7 Table 9
     requires; a repeated keep-alive TesterPresent states ``keep-alive`` again. A request
     classification shall not state both ``keep-alive`` and ``repeat``. At a server, on
     ``T_DataSOM.ind`` and
     ``T_Data.ind`` and, through ``UDSS_LLR_0074``, on the completion report, a request
     classification states no expected response count and no ``repeat``, and may state
     ``keep-alive``, that the message is the functionally addressed TesterPresent whose
     positive response is suppressed, which ISO 14229-1:2020 8.7.6 defines as keep-alive
     logic to be handled by bypass logic;
   * ``final response``, a positive response, or a negative response whose response code
     is not ``requestCorrectlyReceived-ResponsePending``;
   * ``response pending``, a negative response whose response code is
     ``requestCorrectlyReceived-ResponsePending``;
   * ``busy refusal``, a negative response whose response code is ``busyRepeatRequest``,
     refusing a request that ISO 14229-1:2020 8.7.6 kept out of the service in progress
     and that was therefore never indicated as a request.

   A classification whose kind is ``final response`` shall further state whether the
   message is ``solicited``, transmitted because of a request received from a client, or
   ``unsolicited``, transmitted for any other reason.

   A session selection shall state whether the session being selected is the default
   session. It shall be present only where the message effects the transition: a request or
   a positive response that selects a session carries one, and a negative response to a
   session-change request does not. Which service carries the message is immaterial: a
   DiagnosticSessionControl request or positive response is the usual carrier, and an
   ECUReset positive response or the response to an OBD-range request that ISO 14229-1:2020
   8.7.6 has abort the active service and start the default session carries one for the same
   reason, the session layer being unable to tell the services apart under
   ``UDSS_LLR_0073``.

   Rationale: the definition of a final response is ISO 14229-2:2021 9.1.1's; the
   requirement is derived because the classification, not the definition, is this set's
   invention. The same clause settles a case the solicited and unsolicited split exists to
   carry: where a request schedules periodic responses, the initial response accepting or
   refusing the schedule is the final response, and the periodic transmissions that follow
   are not.

   The expected response count is stated by the client and has no server-side counterpart,
   a server answering the one request in front of it. ``UDSS_LLR_0135`` conditions on the
   count being other than ``none``, because ISO 14229-2:2021 10.3 Figure 20 permits a
   client not to start its response timer for a request needing no response, and
   ``UDSS_LLR_0138`` conditions on the exact number, because ISO 14229-2:2021 9.7 Table 9
   makes the client's handling of a response timeout depend on whether it knew how many
   servers would respond. Neither can be determined by the session layer: the count is a
   property of the request the application composed, and ``UDSS_LLR_0073`` forbids reading
   it out of the message.

   The ``keep-alive`` marker is stated by both roles. The client states it because
   ``UDSS_LLR_0157`` conditions on it, ISO 14229-2:2021 9.5 Table 6 restarting the client's
   session timer on the functionally addressed TesterPresent alone, and ``UDSS_LLR_0161``
   because ISO 14229-2:2021 9.7 Table 9 restarts it on the lost response to the physically
   addressed one; ``UDSS_LLR_0073`` forbids recognising either from the data. It is separate
   from the expected response count because in physical keep-alive the TesterPresent may or
   may not require a response. The client session timer document records as an assumption of
   use that the request the application transmits in answer to a keep-alive indication
   carries the marker, in either keep-alive mode.

   The marker names the message's purpose rather than the expiry that prompted it, because
   two uses in the set lie outside the expiry: ``UDSS_LLR_0157`` deliberately acts on a
   marked TesterPresent the application sends while the timer is still running, and
   :doc:`llr-client-error-handling` obliges the application to mark a repeated keep-alive
   ``keep-alive`` again, a repeat being sent because the previous one failed. Defining the
   marker by the expiry would put both outside it. The server's
   caller states the marker because ISO 14229-1:2020 8.7.6 exempts that one message from
   one-request-at-a-time, so it arrives while another service is in progress as conformant
   traffic, which the server's requirements condition on. The session
   layer does not verify the marker against ``S_AI[TAtype]``, the same trust
   ``UDSS_LLR_0176`` extends to ``repeat``: a physically addressed TesterPresent,
   ISO 14229-2:2021 10.1.4.2 Figure 13's, is an ordinary request, and a caller that marks
   one has erred in a way an addressing check would catch only by coincidence. The server
   session timer document records the assumption of use.

   The ``repeat`` marker is stated by the client alone and has no server-side counterpart.
   ``UDSS_LLR_0176`` and ``UDSS_LLR_0177`` condition on it because ISO 14229-2:2021 9.7
   Table 9 caps the client's repeats at two and ``UDSS_LLR_0073`` forbids recognising a
   repeat from the data; it is a declaration the caller makes and the session layer does not
   verify. It is exclusive with ``keep-alive`` because ``UDSS_LLR_0176`` keeps the
   keep-alive outside the repeat count, for the reason given there, and a request that was
   both would have to be counted and not counted at once. The client error handling document
   records as an assumption of use that the application marks each repeat other than of the
   keep-alive TesterPresent ``repeat``, marks a repeated keep-alive TesterPresent
   ``keep-alive`` again, and marks no other request so.

   Kind and session selection are separate because a positive response that selects a
   session is at once a final response and a session selection. A session selection
   accompanies requests as well as responses, because ``UDSS_LLR_0086`` and
   ``UDSS_LLR_0098`` condition on a session-selecting request for which no response is
   transmitted.

   The selection states whether the session is the default one rather than naming the
   session. ``UDSS_LLR_0085``, ``UDSS_LLR_0086``, ``UDSS_LLR_0098`` and ``UDSS_LLR_0100``
   all condition on whether a session is the default, and the session layer has no other
   way to tell: recognising an identifier would mean knowing the ISO 14229-1 encoding, which
   ``UDSS_LLR_0073`` forbids.

   Solicitation is separate from kind because a periodically transmitted positive response
   is at once a final response and unsolicited. It applies only to a final response because
   a response-pending message is by construction a reply to a request, so asking whether it
   was solicited has no meaning.

   A busy refusal is a kind of its own, not a solicited final response, because the request
   it answers never became the service in progress. Addressing is how ``UDSS_LLR_0106``
   matches a response to that service, and with one client the refused request and the
   service in progress share it, so a busy refusal classified as a final response would
   stop the window of the service it was sent to protect and end that service on its
   confirmation. ``UDSS_LLR_0187`` states what the kind does. The response code alone does
   not decide the kind: a handler that answers the service in progress with
   ``busyRepeatRequest`` sends a final response.

.. llr:: An expected response count of zero is rejected
   :id: UDSS_LLR_0066
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   On ``S_Data.req``, a request classification stating as its expected response count an
   exact number of zero shall be rejected as ``UDSS_LLR_0015`` defines. An interface whose
   exact count cannot hold zero satisfies this requirement without a check.

   Rationale: an exact number of zero is rejected rather than read as ``none``, the value
   ``UDSS_LLR_0065`` provides for a request expecting no response, because the two would
   otherwise be two spellings of one value with different behaviour.

.. llr:: A keep-alive with a session selection on a request is rejected
   :id: UDSS_LLR_0067
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   On ``S_Data.req``, a request classification stating ``keep-alive`` together with a
   session selection shall be rejected as ``UDSS_LLR_0015`` defines. An interface in which a
   keep-alive request carries no session selection satisfies this requirement without a
   check.

   Rationale: the ``keep-alive`` marker ``UDSS_LLR_0065`` defines excludes a session
   selection because a TesterPresent changes no session, and because ``UDSS_LLR_0155`` acts
   on the selection and ``UDSS_LLR_0157`` on the marker with different effects on a running
   ``tS3_Client`` timer. The rejection reaches the ``S_Data.req`` because the caller
   composes it, and not an indication, which reports a message already received and which
   ``UDSS_LLR_0036`` forwards; the other input the caller composes is the completion report,
   which ``UDSS_LLR_0068`` covers.

.. llr:: A keep-alive with a session selection on a completion report is rejected
   :id: UDSS_LLR_0068
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   On the completion report of ``UDSS_LLR_0074``, a classification stating ``keep-alive``
   together with a session selection shall be rejected as ``UDSS_LLR_0015`` defines. An
   interface in which a keep-alive completion report carries no session selection satisfies
   this requirement without a check.

   Rationale: the ``keep-alive`` marker ``UDSS_LLR_0065`` defines excludes a session
   selection because a TesterPresent changes no session. The rejection reaches the
   completion report because the caller composes it, and not an indication, which reports a
   message already received and which ``UDSS_LLR_0036`` forwards. Without it, at a server
   ``UDSS_LLR_0096`` and ``UDSS_LLR_0086`` would both claim a completion report so
   classified, with opposite outcomes.

.. llr:: A classification stating no kind where one is required is rejected
   :id: UDSS_LLR_0069
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A ``T_Data.ind`` reporting a successful reception, a ``T_DataSOM.ind``, or a
   ``T_Data.ind`` reporting an unsuccessful reception of a message that was addressed to a
   server, whose classification states no kind, shall be rejected as ``UDSS_LLR_0015``
   defines. A classification's kind shall be absent only where ``UDSS_LLR_0058`` permits it:
   on a ``T_Data.ind`` reporting an unsuccessful reception of a message that was not
   addressed to a server.

   Rationale: no requirement in this set conditions on the kind of a message whose
   reception failed and which was not addressed to a server: ``UDSS_LLR_0136`` and
   ``UDSS_LLR_0137`` act on such a reception by its result alone, and ``UDSS_LLR_0138``,
   ``UDSS_LLR_0144`` and ``UDSS_LLR_0146`` act only on a reception that succeeded, so the
   behaviour is defined whether the kind is stated or not. Stating the exception this way
   keeps the three kind values ``UDSS_LLR_0065`` states a closed set, which every
   requirement conditioning on kind relies on. Every other indication must therefore state
   a kind, and a stated outcome is what makes that checkable — including the server-side
   case ``UDSS_LLR_0058`` does not excuse, which this requirement names rather than merely
   mentioning, since ``UDSS_LLR_0072`` excludes from its own reach every departure this one
   names.

   Every case this requirement reaches but one can be discharged by construction, by an
   interface that makes the kind a required part of each indication obliged to state one.
   Walked against each case in turn. A ``T_DataSOM.ind`` states a kind where its
   classification is a required parameter, which it is in both roles. A ``T_Data.ind``
   reporting a successful reception at a server states one because its classification is
   a required parameter there. A ``T_Data.ind`` reporting an unsuccessful reception of a
   message addressed to a server states one because only a server's own reception
   primitive carries such a message, under ``UDSS_LLR_0030``, and its classification is a
   required parameter there.

   The case that remains is a client's ``T_Data.ind`` reporting a successful reception. It
   is the same primitive on which ``UDSS_LLR_0058`` permits the kind to be omitted, the two
   differing only in their result, so an interface whose data, result and classification
   are separate parameters must admit an absent kind on that primitive and cannot make the
   successful case unwritable. There the first sentence is met by a check: the reception is
   rejected where its result is ``S_OK`` and its classification states no kind. The second
   sentence then holds because the only absence accepted is a client's reception that
   failed, which is a message not addressed to a server and exactly the case
   ``UDSS_LLR_0058`` permits. An interface whose value carrying a client's received data
   also carries its classification, present only on a successful reception, would
   discharge this case by construction instead; either shape satisfies this requirement.

.. llr:: A request at a client stating no expected response count is rejected
   :id: UDSS_LLR_0070
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   At a client, on ``S_Data.req``, a request classification stating no expected response
   count shall be rejected as ``UDSS_LLR_0015`` defines. An interface in which the expected
   response count cannot be omitted satisfies this requirement without a check.

   Rationale: ``UDSS_LLR_0065`` requires a request classification on an ``S_Data.req`` to
   state the expected response count, which without a stated outcome would be an obligation
   on the caller that no test could check. An absent count would let ``UDSS_LLR_0135`` open
   a window that ``UDSS_LLR_0138`` could never close.

.. llr:: A final response stating neither solicited nor unsolicited is rejected
   :id: UDSS_LLR_0071
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A classification whose kind is ``final response`` stating neither ``solicited`` nor
   ``unsolicited`` shall be rejected as ``UDSS_LLR_0015`` defines. An interface in which a
   final response cannot omit it satisfies this requirement without a check.

   Rationale: ``UDSS_LLR_0065`` requires such a classification to state one of the two, and
   without a stated outcome for one that states neither, that obligation on the caller is
   untestable. The rejection introduces no value and no condition beyond the form
   ``UDSS_LLR_0065`` states.

.. llr:: A classification or addressing not of the stated form is rejected
   :id: UDSS_LLR_0072
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A classification or addressing not of the form ``UDSS_LLR_0065``, ``UDSS_LLR_0057``,
   ``UDSS_LLR_0058`` and ``UDSS_LLR_0052`` state for the primitive and the role it arrives
   on, other than a departure ``UDSS_LLR_0066``, ``UDSS_LLR_0067``, ``UDSS_LLR_0068``,
   ``UDSS_LLR_0069``, ``UDSS_LLR_0070`` or ``UDSS_LLR_0071`` names, shall be rejected as
   ``UDSS_LLR_0015`` defines. An interface in which such a form cannot be expressed
   satisfies this without a check.

   Rationale: the sentences of ``UDSS_LLR_0065``, ``UDSS_LLR_0057``, ``UDSS_LLR_0058`` and
   ``UDSS_LLR_0052`` that state a form are otherwise obligations on the caller with no
   stated outcome. ``UDSS_LLR_0066``, ``UDSS_LLR_0067``, ``UDSS_LLR_0068``,
   ``UDSS_LLR_0069``, ``UDSS_LLR_0070`` and ``UDSS_LLR_0071`` state the outcome for the
   particular departures they name, each for the reason given there, and are excluded here
   so that every departure from the stated form is named by exactly one requirement: a
   reader citing one of those cases has one requirement to cite, and narrowing or changing
   the outcome of any of them cannot leave this requirement contradicting it.

   ``UDSS_LLR_0067`` and ``UDSS_LLR_0068`` are among them because a classification stating
   ``keep-alive`` together with a session selection is a departure from ``UDSS_LLR_0065``'s
   form: the ``keep-alive`` marker excludes a session selection, a TesterPresent changing no
   session, which is the reading both of those requirements rest on.

   No residual departure remains once ``UDSS_LLR_0066`` to ``UDSS_LLR_0071`` are excluded,
   so this requirement is discharged by construction in full rather than in part. Walked
   against each source in turn: ``UDSS_LLR_0052``'s address extension travels inside
   ``Mtype``'s ``RDiag`` and ``SecureRDiag`` variants, so it cannot be present on a message
   of a type that carries none, nor absent from one that does. ``UDSS_LLR_0057``'s
   classification is a required parameter of every primitive that carries one, absent only
   where ``UDSS_LLR_0058`` permits it, and that omission is ``UDSS_LLR_0069``'s case, not
   this one. ``UDSS_LLR_0058``'s kind requirement binds a message addressed to a server,
   which only a server's own reception primitive carries under ``UDSS_LLR_0030``, and whose
   classification states kind ``request`` in every case ``UDSS_LLR_0065`` gives it; a
   client's reception primitive, which alone can carry another kind, is never addressed to a
   server. ``UDSS_LLR_0065``'s remaining forms are closed by separate variants, required
   fields or a ``NonZeroU16``, save one: a session selection's presence is tied to whether
   the message effects a session transition, a fact ``UDSS_LLR_0073`` forbids the session
   layer from determining. The session layer can therefore never recognise an input as
   departing from that one form, so no departure from it is an input this requirement can
   reach either, and the closed set of remaining forms leaves nothing else for it to reach.

.. llr:: The session layer does not inspect message data
   :id: UDSS_LLR_0073
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall not interpret the contents of ``S_Data`` or ``T_Data``. For any
   two inputs differing only in that message data, the session layer shall produce outputs
   that are identical except for the message data forwarded between the application and
   the transport layer.

   Rationale: this is what makes ``UDSS_LLR_0057`` enforceable rather than aspirational.
   Without it a classification could be supplied and then quietly second-guessed by
   parsing, and the crate would acquire a dependency on the application layer encodings
   that no requirement records.

   The requirement is stated as an equivalence over pairs of inputs rather than as a
   prohibition on reading, because the session layer must necessarily handle those bytes
   in order to forward them: ``UDSS_LLR_0033`` requires it to transmit ``S_Length`` bytes
   of ``S_Data``, and ``UDSS_LLR_0047`` maps ``S_Data`` onto ``T_Data``. The equivalence
   admits a direct test: vary the payload, hold everything else, and compare the outputs.

.. llr:: Completion of a request with no response is reported by the caller
   :id: UDSS_LLR_0074
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall accept from the caller an input reporting that the handling of a
   received request is complete and that no response message will be transmitted. That input
   shall carry the addressing information of the request and the message classification that
   accompanied it.

   Rationale: ISO 14229-2:2021 9.5 Table 6 gives completion of the requested action, where
   no response is required or allowed, as a condition that restarts ``tS3_Server``. No
   message is transmitted in that case, so there is no ``T_Data.conf`` to observe and no
   message classification that could carry the fact. Without an explicit input the session
   layer cannot detect it, and a server handling a suppressed-response request in a
   non-default session would never restart its timer. ISO 14229-2:2021 10.1.4.1 bounds when
   that completion occurs, a service being in progress until the completion of any action
   caused by the request where no response is required, the point that would otherwise have
   started the response.

   The classification is carried on the input rather than recovered by correlating it with
   an earlier ``T_Data.ind``, because Table 6's other suppressed-response row, the
   transition from the default session to a non-default one, needs the session selection,
   and correlating would oblige the session layer to retain one. The report identifies its
   request by addressing alone, no requirement acting on it asking more than whose request
   completed.
